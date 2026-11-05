#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
//
// Thin wrapper around the two-host LAN leg driver (transports/zenoh/examples/lan_leg.rs):
// builds it, runs it while keeping its evidence lines in one file per step, stamps manual
// actions, and assembles both hosts' evidence into a pass/fail table. The procedure is the
// oac-lan-leg skill (.claude/skills/oac-lan-leg/SKILL.md). Node built-ins only.
//
//   node tools/lan-leg/lan-leg.mjs build
//   node tools/lan-leg/lan-leg.mjs host     --dir D --role a|b [--firewall "<policy>"]
//   node tools/lan-leg/lan-leg.mjs run      --dir D --label L -- <lan_leg arguments>
//   node tools/lan-leg/lan-leg.mjs mark     --dir D --label L     (a manual action, now)
//   node tools/lan-leg/lan-leg.mjs assemble --dir D [--b-dir D2]
//   node tools/lan-leg/lan-leg.mjs selftest [--port-base N]       (both roles on 127.0.0.1)
//
// It never sets OAC_TEST_LAN: the opt-in stays the operator's. It writes evidence only
// under <dir>/evidence (no key material), and never reads a harness credential file.

import { spawn, spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync, appendFileSync } from 'node:fs';
import { networkInterfaces, release, tmpdir, type as osType } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
const SCHEMA = 'oac-lan-leg/1';
const LEASE_MS = 10_000;
const SKEW_LIMIT_MS = 5_000;
const SILENT_SLACK_MS = 5_000; // "the lease plus a few seconds of scheduling" (README step 8)
const BIN = process.env.LAN_LEG_BIN ?? join(repoRoot, 'target', 'debug', 'examples', process.platform === 'win32' ? 'lan_leg.exe' : 'lan_leg');

const die = (m, code = 2) => { console.error(`lan-leg: ${m}`); process.exit(code); };

function parse(argv) {
  const out = { _: [] };
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--') { out.rest = argv.slice(i + 1); break; }
    if (argv[i].startsWith('--')) out[argv[i].slice(2)] = argv[++i] ?? die(`${argv[i - 1]} needs a value`);
    else out._.push(argv[i]);
  }
  return out;
}

const evidenceDir = (dir) => join(resolve(dir ?? die('--dir is required')), 'evidence');
function record(dir, label, obj) {
  mkdirSync(evidenceDir(dir), { recursive: true });
  const line = JSON.stringify({ schema: SCHEMA, ...obj });
  appendFileSync(join(evidenceDir(dir), `${label}.jsonl`), `${line}\n`);
  console.log(line);
}
const nowUs = () => Date.now() * 1000;
const iso = (us) => new Date(us / 1000).toISOString();

// ---- build / run / mark / host ---------------------------------------------------------

function build() {
  const r = spawnSync('cargo', ['build', '-p', 'oac-transport-zenoh', '--example', 'lan_leg', '--locked'], { cwd: repoRoot, stdio: 'inherit' });
  process.exit(r.status ?? 1);
}

function run(a) {
  if (process.env.OAC_TEST_LAN !== '1') die('refused: set OAC_TEST_LAN=1 (the LAN opt-in is the operator\'s; this wrapper never sets it)', 3);
  if (!a.label || !a.rest) die('usage: run --dir D --label L -- <lan_leg arguments>');
  if (!existsSync(BIN)) die(`driver not built: ${BIN} (run: node tools/lan-leg/lan-leg.mjs build)`);
  mkdirSync(evidenceDir(a.dir), { recursive: true });
  const file = join(evidenceDir(a.dir), `${a.label}.jsonl`);
  const child = spawn(BIN, a.rest, { stdio: ['ignore', 'pipe', 'inherit'], env: process.env });
  let buf = '';
  child.stdout.on('data', (d) => {
    buf += d;
    let nl;
    while ((nl = buf.indexOf('\n')) >= 0) {
      const line = buf.slice(0, nl + 1);
      buf = buf.slice(nl + 1);
      appendFileSync(file, line);
      process.stdout.write(line);
    }
  });
  const stop = () => child.kill();
  process.on('SIGINT', stop);
  process.on('SIGTERM', stop);
  child.on('exit', (code, signal) => {
    if (buf) { appendFileSync(file, `${buf}\n`); process.stdout.write(`${buf}\n`); }
    const end = { event: 'wrapper-exit', label: a.label, exit_code: code, signal, utc: iso(nowUs()), utc_us: nowUs() };
    appendFileSync(file, `${JSON.stringify({ schema: SCHEMA, ...end })}\n`);
    process.exit(code ?? 1);
  });
}

function mark(a) {
  if (process.env.OAC_TEST_LAN !== '1') die('refused: set OAC_TEST_LAN=1', 3);
  const at = nowUs();
  record(a.dir, a.label ?? die('--label is required'), { event: 'action', role: a.role ?? 'b', label: a.action ?? a.label, utc: iso(at), utc_us: at, by: 'operator-mark' });
}

function psJson(cmd) {
  const r = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', cmd], { encoding: 'utf8' });
  try { return JSON.parse(r.stdout); } catch { return null; }
}

function host(a) {
  const at = nowUs();
  const ips = Object.entries(networkInterfaces()).flatMap(([name, list]) => (list ?? []).filter((i) => i.family === 'IPv4' && !i.internal).map((i) => ({ interface: name, address: i.address })));
  const o = { event: 'host', role: a.role ?? die('--role is required'), os: osType(), os_release: release(), platform: process.platform,
    utc: iso(at), utc_us: at, utc_offset_minutes: -new Date().getTimezoneOffset(), ipv4: ips,
    firewall_policy_stated_by_operator: a.firewall ?? null };
  if (process.platform === 'win32') {
    // Read-only queries; nothing is changed.
    o.firewall_profiles = psJson('Get-NetFirewallProfile | Select-Object Name,Enabled,DefaultInboundAction | ConvertTo-Json -Compress');
    o.network_category = psJson('Get-NetConnectionProfile | Select-Object InterfaceAlias,NetworkCategory | ConvertTo-Json -Compress');
  }
  record(a.dir, `host-${o.role}`, o);
}

// ---- assemble --------------------------------------------------------------------------

function load(dir) {
  const m = new Map();
  const d = evidenceDir(dir);
  if (!existsSync(d)) return m;
  for (const f of readdirSync(d).filter((n) => n.endsWith('.jsonl'))) {
    const evs = [];
    for (const l of readFileSync(join(d, f), 'utf8').split('\n')) {
      try { const o = JSON.parse(l); if (o && o.event) evs.push(o); } catch { /* not an evidence line */ }
    }
    m.set(f.slice(0, -6), evs);
  }
  return m;
}

export function assemble(A, B) {
  const get = (m, l) => m.get(l) ?? [];
  const of = (evs, e) => evs.filter((x) => x.event === e);
  const crit = [];
  const add = (id, step, text, result, detail) => crit.push({ id, step, text, result, detail });
  const all = (m) => [...m.values()].flat();
  const status = (ok) => (ok ? 'PASS' : 'FAIL');
  const NOT_RUN = 'NOT RUN';

  // Step 1: versions, hosts, clock.
  const envA = of(all(A), 'env')[0], envB = of(all(B), 'env')[0];
  const hostA = get(A, 'host-a')[0], hostB = get(B, 'host-b')[0] ?? get(A, 'host-b')[0];
  if (!envA || !envB) add('1.versions', 1, 'same OAC commit and Zenoh version on both hosts', NOT_RUN, 'no env line from one host');
  else {
    const shaA = envA.oac_commit?.sha, shaB = envB.oac_commit?.sha;
    const same = shaA && shaA === shaB && envA.zenoh_version === envB.zenoh_version;
    const dirty = envA.oac_commit?.tree_dirty || envB.oac_commit?.tree_dirty;
    add('1.versions', 1, 'same OAC commit and Zenoh version on both hosts, clean trees', status(same && !dirty),
      `A ${envA.os} ${shaA} ${envA.zenoh_version} dirty=${envA.oac_commit?.tree_dirty}; B ${envB.os} ${shaB} ${envB.zenoh_version} dirty=${envB.oac_commit?.tree_dirty}`);
  }
  add('1.hosts', 1, 'OS, IPs, firewall policy and UTC offset recorded for both hosts', hostA && hostB ? 'PASS' : NOT_RUN,
    hostA && hostB ? `A offset ${hostA.utc_offset_minutes} min, fw "${hostA.firewall_policy_stated_by_operator}"; B offset ${hostB.utc_offset_minutes} min, fw "${hostB.firewall_policy_stated_by_operator}"` : 'run the host command on each host');
  // Every skew batch counts: a batch with no sample is a failure, not a dropped row.
  const batches = [...all(A), ...all(B)].filter((e) => e.event === 'skew');
  const okBatch = (s) => s.samples > 0 && s.best_offset_us != null && Math.abs(s.best_offset_us) <= SKEW_LIMIT_MS * 1000;
  const worstMs = Math.max(0, ...batches.filter((s) => s.best_offset_us != null).map((s) => Math.abs(s.best_offset_us) / 1000));
  add('1.clock', 1, `measured wall-clock skew at most ${SKEW_LIMIT_MS} ms in every batch`, batches.length ? status(batches.every(okBatch)) : NOT_RUN,
    batches.length ? `${batches.length} batch(es), ${batches.filter((s) => !(s.samples > 0)).length} without a sample (fail); worst ${worstMs} ms (min-delay sample of each batch, NTP-style over the carrier)` : 'no skew measurement');

  // Step 2: pairing.
  const iss = of(get(A, 'step2-issue-a'), 'issue')[0], imp = of(get(B, 'step2-import-b'), 'import')[0];
  add('2.issue', 2, 'authority issued per-host credentials without persisting its key; the bundle for B imported owner-only',
    iss && imp ? status(iss.authority_private_key_persisted === false && imp.hardened.length >= 5) : NOT_RUN,
    iss && imp ? `authority key persisted=${iss.authority_private_key_persisted}; CNs ${iss.cn_a}, ${iss.cn_b}, ${iss.cn_c}; hardened on B: ${imp.hardened.join(', ')}` : 'no issue/import evidence');
  add('2.no-host-key-distribution', 2, 'never distribute a host key to other peers (README step 2)', NOT_RUN,
    "deviation, test-only: the API has no CSR form, so B's and C's private keys are generated on A and carried by the operator out of band");

  // Steps 3 and 4: frames both ways.
  const twoWay = (id, step, text, la, lb, extra) => {
    const a = get(A, la), b = get(B, lb);
    if (!a.length || !b.length) return add(id, step, text, NOT_RUN, `missing evidence ${la} / ${lb}`);
    const okStart = of(a, 'start')[0]?.ok === true && of(b, 'start')[0]?.ok === true;
    const aGot = of(a, 'frame').filter((f) => f.verdict === 'accepted' && f.from === 'b').length;
    const bGot = of(b, 'frame').filter((f) => f.verdict === 'accepted' && f.from === 'a').length;
    const x = extra ? extra(a, b) : { ok: true, note: '' };
    add(id, step, text, status(okStart && aGot > 0 && bGot > 0 && x.ok), `starts ok=${okStart}; A accepted ${aGot} frame(s) from B; B accepted ${bGot} from A${x.note}`);
  };
  twoWay('3.explicit', 3, 'explicit endpoints, scouting off: sealed frames carried both ways', 'step3-a', 'step3-b',
    (a, b) => { const s = of(a, 'start')[0], t = of(b, 'start')[0]; return { ok: s.scouting === 'off' && t.scouting === 'off', note: `; scouting ${s.scouting}/${t.scouting}` }; });
  add('3.core-signed', 3, 'core-signed HPKE frames in both directions', NOT_RUN, 'synthetic frames sealed with the RFC 7748 test keys through oac_core::sealing, not core-signed');
  add('3.contract', 3, 'unchanged transport-contract checks through a two-host binding harness', NOT_RUN, 'the shared suite needs an in-process Medium; no two-host harness at this commit');
  twoWay('4.multicast', 4, 'multicast scouting, no explicit remote endpoints: discovery and frames both ways', 'step4-mc-a', 'step4-mc-b',
    (a, b) => { const s = of(a, 'start')[0], t = of(b, 'start')[0]; const disc = of(a, 'presence').some((p) => p.kind === 'appeared') && of(b, 'presence').some((p) => p.kind === 'appeared');
      return { ok: s.scouting === 'multicast' && t.scouting === 'multicast' && !s.connect && !t.connect && disc, note: `; scouting ${s.scouting}/${t.scouting}, remote endpoints ${s.connect ?? 'none'}/${t.connect ?? 'none'}, discovery event on both=${disc}` }; });
  // The fallback only means something if multicast is shown to be blocked: the same
  // multicast peers, no explicit endpoints, must not link while the operator's block is in place.
  const ca = get(A, 'step4-fb-check-a'), cb = get(B, 'step4-fb-check-b');
  const blocked = ca.length && cb.length
    ? of(ca, 'start')[0]?.scouting === 'multicast' && of(cb, 'start')[0]?.scouting === 'multicast' && !of(ca, 'start')[0]?.connect && !of(cb, 'start')[0]?.connect
      && of(ca, 'link')[0]?.up === false && of(cb, 'link')[0]?.up === false && !of(ca, 'presence').some((p) => p.kind === 'appeared') && !of(cb, 'presence').some((p) => p.kind === 'appeared')
    : null;
  if (blocked === null) add('4.fallback', 4, 'multicast blocked at the network (shown), step 3 repeated (G3 fallback)', NOT_RUN, 'no step4-fb-check evidence: the block is operator-stated, unverified');
  else if (!blocked) add('4.fallback', 4, 'multicast blocked at the network (shown), step 3 repeated (G3 fallback)', 'FAIL', 'the multicast check peers linked (or did not start) while the block should be in place: the condition is not established');
  else twoWay('4.fallback', 4, 'multicast blocked at the network (shown: multicast peers did not link), step 3 repeated (G3 fallback)', 'step4-fb-a', 'step4-fb-b');
  add('4.tls-only', 4, 'TLS-only links', NOT_RUN, 'configuration only (native protocol allowlist is tls); links not inspected');

  // Step 5: negative controls. A refusal counts only beside a passing positive control on
  // the same --listen and --connect, with host A's port answering.
  const ctl = of(get(B, 'step5-control-b'), 'negative').find((n) => n.case === 'control');
  const ctlFor = (n) => !!ctl && ctl.pass === true && ctl.listen === n.listen && ctl.connect === n.connect;
  for (const c of ['wrong-ca', 'wrong-san']) {
    const n = of(get(B, `step5-${c}-b`), 'negative')[0];
    const served = c !== 'wrong-san' || of(get(A, 'step5-wrong-san-a'), 'start')[0]?.ok === true; // A must serve its wrong-SAN credential
    add(`5.${c}`, 5, `${c}: refused by the peer's native authentication`, n && ctl ? status(n.pass === true && ctlFor(n) && served) : NOT_RUN,
      n ? `start_ok=${n.start_ok}; listener_reachable=${n.listener_reachable}; native_open_failed=${n.native_open_failed}; control linked=${ctl?.linked ?? 'missing'} on the same listen/connect=${ctl ? ctlFor(n) || ctl.pass === false : false}; A served wrong-SAN credential=${served}; ${n.error ?? ''}` : 'no evidence');
  }
  const wk = of(get(B, 'step5-wrong-key-b'), 'negative')[0];
  add('5.wrong-key', 5, "wrong key refused by the peer's native authentication", NOT_RUN,
    `cannot be shown with this API (one file set serves both the local listener and the connection, so the local native stack can refuse the mismatch first)${wk ? `; observed start_ok=${wk.start_ok}: ${wk.error ?? ''}; recorded, not counted` : ''}`);
  const un = of(get(B, 'step5-unlisted-b'), 'negative')[0];
  const leaked = of(get(A, 'step5-a'), 'frame').filter((f) => f.tag === 'neg-probe' && f.verdict === 'accepted').length;
  const unOk = un && ((un.start_ok && un.probes_sent > 0) || (!un.start_ok && un.native_open_failed && un.listener_reachable && ctlFor(un)));
  add('5.unlisted-cn', 5, 'CA-valid unlisted CN: no frame carried through the ACL', un && get(A, 'step5-a').length ? status(leaked === 0 && unOk) : NOT_RUN,
    un ? `link opened=${un.start_ok}; probes sent ${un.probes_sent}; accepted on A ${leaked}` : 'no evidence');
  add('5.absent-client-cert', 5, 'absent client certificate refused', NOT_RUN, 'no public API starts a peer without a client certificate at this commit; loopback control only');
  add('5.plaintext', 5, 'plaintext discovered locator refused', NOT_RUN, 'needs a raw native plaintext peer; not driven at this commit');

  // Step 6: delayed receipt beyond the encrypted deadline.
  const s6a = get(A, 'step6-a');
  const t6 = '(driver stand-in for SEC-SEL-036) a frame delayed beyond its encrypted deadline is discarded on receipt; native arrival recorded';
  if (!s6a.length) add('6.expiry', 6, t6, NOT_RUN, 'no step6-a evidence');
  else {
    const fr = of(s6a, 'frame');
    const ontime = fr.find((f) => f.tag === 'ontime'), late = fr.find((f) => f.tag === 'late');
    const ok = ontime?.verdict === 'accepted' && late?.verdict === 'discard-expired' && late.native_utc_us != null;
    add('6.expiry', 6, t6, status(!!ok),
      `ontime ${ontime?.verdict ?? 'missing'}; late ${late?.verdict ?? 'missing'} (opened ${late?.late_by_ms ?? '?'} ms past its deadline, native arrival ${late ? iso(late.native_utc_us) : '?'}). The discard is the driver's own check, not the core pipeline.`);
  }
  add('6.core-discard', 6, 'receiving-core discard (SEC-SEL-036) of the delayed frame', NOT_RUN, 'the core receive pipeline is not run by the driver; see 6.expiry for the stand-in');
  const s6b = [...get(B, 'step6-b')].filter((e) => e.event === 'skew');
  add('6.skew-before-after', 6, 'skew recorded before and after the short-deadline exchange, against the 5 s limit',
    s6b.some((s) => s.phase === 'before') && s6b.some((s) => s.phase === 'after') ? status(s6b.every(okBatch)) : NOT_RUN,
    s6b.map((s) => `${s.phase}: ${s.samples > 0 ? s.abs_skew_ms + ' ms' : 'no sample'}`).join('; ') || 'no evidence');
  add('7.relay-replace', 7, 'replace the local relay with another same-user process; reconnect; refuse plain holder', NOT_RUN, 'needs the CLI daemon (not built at this commit)');

  // Step 8: presence and carrier loss. Offset (A clock minus B clock) and its error come
  // from the step-8 skew batch that B ran on its own link to A.
  const aEv = get(A, 'step8-a'), bEv = get(B, 'step8-b');
  const s8 = of(bEv, 'skew').find((s) => s.local === 'b' && s.phase === 'before' && s.samples > 0);
  const offsetUs = s8 ? s8.best_offset_us : null;
  const errMs = s8 ? s8.best_delay_us / 2000 : null;
  const pm = (v) => `${v} ms (+-${errMs} ms: half the batch's best round trip)`;
  const cRuns = [['clean', 'step8-c-clean-b'], ['abort', 'step8-c-abort-b'], ['silent', 'step8-c-silent-b']];
  const joined = cRuns.map(([k, l]) => ({ k, evs: get(B, l) })).filter((r) => r.evs.length);
  add('8.late-join', 8, 'a third transport started late learns of the first two from native liveliness history',
    joined.length && offsetUs != null ? status(joined.every((r) => {
      const st = of(r.evs, 'start')[0];
      const firstAp = of(r.evs, 'presence').find((p) => p.kind === 'appeared');
      const firstSend = of(r.evs, 'send')[0];
      const history = st?.ok && firstAp && (!firstSend || firstAp.utc_us < firstSend.utc_us) && !of(r.evs, 'frame').some((f) => f.utc_us < firstAp.utc_us);
      const seenA = of(aEv, 'presence').some((p) => p.kind === 'appeared' && p.utc_us >= st.utc_us + offsetUs - errMs * 1000);
      const seenB = of(bEv, 'presence').some((p) => p.kind === 'appeared' && p.utc_us >= st.utc_us);
      return history && of(r.evs, 'probe').some((p) => p.linked_to_both) && seenA && seenB;
    })) : NOT_RUN,
    (joined.length ? joined.map((r) => `${r.k}: ${of(r.evs, 'presence').filter((p) => p.kind === 'appeared').length} appearance event(s) on C (first after ${of(r.evs, 'presence').find((p) => p.kind === 'appeared')?.since_start_ms ?? '?'} ms) before any frame; pongs from A=${of(r.evs, 'probe')[0]?.pongs_from_a ?? '?'}, from B=${of(r.evs, 'probe')[0]?.pongs_from_b ?? '?'}`).join('; ') : 'no third-transport evidence') +
    '. The transport coalesces appearances into one event per carrier handle, so two distinct appearances cannot be required; "the first two" is shown by the pongs from both.');
  const marksB = get(B, 'step8-marks-b');
  const BOUND = LEASE_MS + SILENT_SLACK_MS;
  for (const [k, l] of cRuns) {
    const evs = get(B, l);
    const pre = k === 'silent' ? of(marksB, 'action').find((x) => x.label === 'silent-loss-pre') : of(evs, 'action').find((x) => x.label === `end-${k}`);
    const post = k === 'silent' ? of(marksB, 'action').find((x) => x.label === 'silent-loss-post') : null;
    const id = `8.loss-${k}`;
    const text = k === 'silent'
      ? `silent loss: carrier-loss reported on the other host within the lease plus scheduling (${BOUND} ms; README step 8)`
      : `${k} stop: carrier-loss reported on the other host within ${BOUND} ms (the assembler's own bound, not the README's; it states one for the silent loss only)`;
    if (!pre || !aEv.length || offsetUs == null) { add(id, 8, text, NOT_RUN, 'no action instant, no observer evidence or no step-8 skew batch'); continue; }
    const base = pre.utc_us + offsetUs; // the action, on A's clock
    const losses = of(aEv, 'presence').filter((p) => p.kind === 'loss' && p.utc_us >= base - errMs * 1000);
    const lossA = losses[0];
    const rawA = lossA ? (lossA.utc_us - pre.utc_us) / 1000 : null;
    const corrA = lossA ? (lossA.utc_us - base) / 1000 : null;
    const ok = corrA != null && corrA >= -errMs && corrA <= BOUND;
    const inWindow = losses.filter((p) => p.utc_us <= base + BOUND * 1000);
    const lossB = of(bEv, 'presence').find((p) => p.kind === 'loss' && p.utc_us >= pre.utc_us);
    const fromPost = post && lossA ? ` (from the post-drop mark: ${(lossA.utc_us - (post.utc_us + offsetUs)) / 1000} ms)` : '';
    add(id, 8, text, status(ok),
      `action ${iso(pre.utc_us)} on B${k === 'silent' ? ' (pre-drop mark; bound checked from it)' : ''}; first loss on A ${lossA ? iso(lossA.utc_us) : 'none seen'}; raw ${rawA} ms, skew-corrected ${corrA == null ? 'n/a' : pm(corrA)}${fromPost}; offset A-B ${offsetUs / 1000} ms; ${inWindow.length} loss event(s) on A in the window (handle index ${lossA?.handle_index ?? '?'}: one handle for all transports, so attribution is by time)${k === 'silent' ? '' : `; under the lease=${corrA != null && corrA < LEASE_MS}`}${lossB ? `; same-host observer on B ${(lossB.utc_us - pre.utc_us) / 1000} ms` : ''}`);
  }
  return crit;
}

function render(crit, A, B) {
  const env = [...A.values(), ...B.values()].flat().find((e) => e.event === 'env');
  const pass = crit.filter((c) => c.result === 'PASS').length, fail = crit.filter((c) => c.result === 'FAIL').length, nr = crit.filter((c) => c.result === 'NOT RUN').length;
  const verdict = fail ? 'FAIL' : nr ? 'NOT RUN' : 'PASS';
  const first = Math.min(...[...A.values(), ...B.values()].flat().map((e) => e.utc_us).filter(Boolean));
  const box = { PASS: '[x]', FAIL: '[f]', 'NOT RUN': '[ ]' };
  const md = [
    '### G3 two-host LAN leg (#64 steps 1-7, #63 step 8)', '',
    '- **Gate id:** G3',
    `- **Pinned version(s):** Zenoh ${env?.zenoh_version ?? '?'}; OAC ${env?.oac_commit?.sha ?? '?'}`,
    `- **Date:** ${Number.isFinite(first) ? iso(first).slice(0, 10) : '?'}`,
    '- **Timebox:** FILL: the box set / elapsed',
    '- **Command transcript summary:** driver evidence lines per step (JSON, tools/lan-leg); see the table below',
    '- **Pass criteria evaluated:**',
    ...crit.map((c) => `  - ${box[c.result]} ${c.step}/${c.id}: ${c.text} -- ${c.result}: ${c.detail}`),
    `- **Verdict:** ${verdict === 'NOT RUN' ? 'NOT RUN (' + nr + ' criteria not run; ' + pass + ' passed)' : verdict}`,
    '- **Fallback taken:** FILL: none, or the multicast-blocked fallback (4.fallback)',
    '- **UNVERIFIED items:** FILL: still open unless closed by the rows above',
    '- **Fixtures captured:** none committed; evidence files held by the operator',
    '- **Human actions:** FILL: firewall rules added/removed, cable or packet-drop action, bundle carried (who)',
  ].join('\n');
  return { verdict, pass, fail, notRun: nr, md };
}

function assembleCmd(a) {
  const A = load(a.dir), B = a['b-dir'] ? load(a['b-dir']) : A;
  const crit = assemble(A, B);
  const r = render(crit, A, B);
  mkdirSync(resolve(a.dir), { recursive: true });
  writeFileSync(join(resolve(a.dir), 'lan-leg-summary.json'), `${JSON.stringify({ schema: SCHEMA, verdict: r.verdict, criteria: crit }, null, 2)}\n`);
  writeFileSync(join(resolve(a.dir), 'lan-leg-gate-result.md'), `${r.md}\n`);
  console.log(JSON.stringify({ schema: SCHEMA, event: 'assembled', verdict: r.verdict, pass: r.pass, fail: r.fail, not_run: r.notRun }));
  for (const c of crit) console.log(`${c.result.padEnd(7)} ${c.step}/${c.id}  ${c.detail}`);
  return r;
}

// ---- selftest: both roles on 127.0.0.1, distinct ports ----------------------------------

function selftest(a) {
  if (process.env.OAC_TEST_LAN !== '1') die('refused: set OAC_TEST_LAN=1', 3);
  if (!existsSync(BIN)) die(`driver not built: ${BIN}`);
  const base = Number(a['port-base'] ?? 40000 + (process.pid % 2000) * 4);
  const pa = `127.0.0.1:${base}`, pb = `127.0.0.1:${base + 1}`, pc = `127.0.0.1:${base + 2}`;
  const root = a.dir ? resolve(a.dir) : mkdtempSync(join(tmpdir(), 'oac-lan-leg-selftest-'));
  const dirA = join(root, 'host-a-side'), dirB = join(root, 'host-b-side');
  const sh = (args) => new Promise((res) => {
    const c = spawn(BIN, args, { env: process.env, stdio: ['ignore', 'pipe', 'inherit'] });
    c.stdout.on('data', () => {});
    c.on('exit', res);
  });
  // A driver process with its evidence kept under `dir`.
  const wrap = (dir, label, args) => new Promise((res) => {
    const c = spawn(process.execPath, [fileURLToPath(import.meta.url), 'run', '--dir', dir, '--label', label, '--', ...args], { env: process.env, stdio: ['ignore', 'ignore', 'inherit'] });
    c.on('exit', res);
  });
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const asyncMain = async () => {
    console.log(`selftest: loopback only, ports ${base}..${base + 2}, state under ${root}`);
    mkdirSync(dirA, { recursive: true });
    record(dirA, 'host-a', { event: 'host', role: 'a', platform: process.platform, utc_offset_minutes: -new Date().getTimezoneOffset(), utc: iso(nowUs()), utc_us: nowUs(), firewall_policy_stated_by_operator: 'loopback dry run: no LAN listener' });
    mkdirSync(dirB, { recursive: true });
    record(dirB, 'host-b', { event: 'host', role: 'b', platform: process.platform, utc_offset_minutes: -new Date().getTimezoneOffset(), utc: iso(nowUs()), utc_us: nowUs(), firewall_policy_stated_by_operator: 'loopback dry run: no LAN listener' });
    // Step 2: issue on A; carry for-host-b to B as a plain copy (modes lost on purpose), import.
    await wrap(dirA, 'step2-issue-a', ['issue', '--dir', dirA, '--ip-a', '127.0.0.1', '--ip-b', '127.0.0.1']);
    const iss = load(dirA).get('step2-issue-a')?.find((e) => e.event === 'issue');
    if (!iss) die('selftest: issue failed', 1);
    cpSync(join(dirA, 'for-host-b'), join(dirB, 'for-host-b'), { recursive: true });
    await wrap(dirB, 'step2-import-b', ['import', '--dir', dirB]);
    const allowA = `${iss.cn_b},${iss.cn_c}`, allowB = `${iss.cn_a},${iss.cn_c}`;
    const peerA = (label, secs, extra = []) => wrap(dirA, label, ['peer', '--dir', dirA, '--cred', 'a', '--role', 'a', '--listen', pa, '--allow', allowA, '--run-secs', String(secs), ...extra]);
    const peerB = (label, secs, extra = []) => wrap(dirB, label, ['peer', '--dir', dirB, '--cred', 'b', '--role', 'b', '--listen', pb, '--connect', pa, '--allow', `${iss.cn_a},${iss.cn_c}`, '--run-secs', String(secs), ...extra]);
    // Steps 1 and 3.
    console.log('selftest: steps 1, 3');
    let pA = peerA('step3-a', 16);
    await sleep(2500);
    await peerB('step3-b', 6, ['--skew-pings', '6']);
    await pA;
    // Step 6.
    console.log('selftest: step 6');
    pA = peerA('step6-a', 24);
    await sleep(2500);
    await peerB('step6-b', 3, ['--skew-pings', '6', '--timed-test', '--skew-pings-after', '6']);
    await pA;
    // Step 5.
    console.log('selftest: step 5');
    pA = peerA('step5-a', 42);
    await sleep(2500);
    await wrap(dirB, 'step5-control-b', ['negative', '--dir', dirB, '--case', 'control', '--role', 'b', '--listen', pb, '--connect', pa, '--allow', iss.cn_a]);
    for (const c of ['wrong-ca', 'wrong-key']) await wrap(dirB, `step5-${c}-b`, ['negative', '--dir', dirB, '--case', c, '--role', 'b', '--listen', pb, '--connect', pa, '--allow', iss.cn_a]);
    await wrap(dirB, 'step5-unlisted-b', ['negative', '--dir', dirB, '--case', 'unlisted-cn', '--role', 'b', '--listen', pb, '--connect', pa, '--allow', iss.cn_a, '--probes', '4']);
    await pA;
    const wsA = wrap(dirA, 'step5-wrong-san-a', ['peer', '--dir', dirA, '--cred', 'neg-wrong-san', '--role', 'a', '--listen', pa, '--allow', allowA, '--run-secs', '14']);
    await sleep(2500);
    await wrap(dirB, 'step5-wrong-san-b', ['negative', '--dir', dirB, '--case', 'wrong-san', '--role', 'b', '--listen', pb, '--connect', pa, '--allow', iss.cn_a]);
    await wsA;
    // Step 8 (clean and killed; a silent loss needs a packet drop, so it is not run here).
    console.log('selftest: step 8');
    pA = peerA('step8-a', 60);
    await sleep(2500);
    const pB = peerB('step8-b', 50, ['--skew-pings', '6']);
    await sleep(6000);
    for (const end of ['clean', 'abort']) {
      await wrap(dirB, `step8-c-${end}-b`, ['peer', '--dir', dirB, '--cred', 'c', '--role', 'c', '--listen', pc, '--connect', `${pa},${pb}`, '--allow', `${iss.cn_a},${iss.cn_b}`, '--probe-peers', '--run-secs', '4', '--end', end]);
      await sleep(3000);
    }
    await pB; await pA;
    console.log('selftest: assembling (step 4, silent loss, step 7 and the criteria the driver cannot show are NOT RUN on loopback)');
    const r = assembleCmd({ dir: dirA, 'b-dir': dirB });
    if (!a.dir) rmSync(root, { recursive: true, force: true });
    process.exit(r.fail ? 1 : 0);
  };
  asyncMain();
}

// ---- main ------------------------------------------------------------------------------

if (resolve(process.argv[1] ?? '') === fileURLToPath(import.meta.url)) {
  const [cmd, ...rest] = process.argv.slice(2);
  const a = parse(rest);
  switch (cmd) {
    case 'build': build(); break;
    case 'host': host(a); break;
    case 'run': run(a); break;
    case 'mark': mark(a); break;
    case 'assemble': assembleCmd(a); break;
    case 'selftest': selftest(a); break;
    default: die('usage: build | host | run | mark | assemble | selftest (see the header of this file)');
  }
}
