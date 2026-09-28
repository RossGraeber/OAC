// K7 (#130) tests, run by the driver self-test (`node tools/herdr/run.mjs --self-test`).
//
// Unit half: the Codex pin-move check (lib/pins.mjs), lib/g2.mjs against the REAL committed
// human-run G2 fixtures (the 0.157.1 re-run and the 0.154.0 baseline) and synthetic
// variants, the fixture sanitizer, the pane-process argv proof, client staging, and the
// scoring and write rules of lib/g2-report.mjs.
//
// Lifecycle half: tools/herdr/scenarios/g2-codex-inject.mjs end to end through run.mjs
// against test/fake-herdr.mjs and test/fake-codex.mjs, both TEST DOUBLES, with the REAL
// committed G2 client staged and run unmodified. One case runs under test/fs-trace.mjs and
// checks, from the trace, that neither the driver nor the client opened anything under the
// Codex home beyond the two harness-config files run.mjs hashes, and what each of them
// started. These prove the scenario's and the driver's own behavior. They prove nothing
// about herdr or Codex: a live G2 run through herdr is UNVERIFIED until an operator runs it.

import { spawnSync } from 'node:child_process';
import { chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, relative, resolve, isAbsolute } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { parseCodexLastObserved, parseCodexCliVersion, parseCodexUserAgentVersion, parseCodexDaemonVersion, codexPinMoveTrigger } from '../lib/pins.mjs';
import {
  BASELINE_TRANSCRIPT, COMMITTED_CLIENT, COMMITTED_CLIENT_SHA256, FIXTURE_DIR, G2_LAUNCH, MANIFEST_PATH, DEFAULT_OPERATOR_PROMPT, assertNotInjected, classifyCodexScreen,
  codexLaunchProof, compareByMode, driverMayAcceptCodex, fixtureNames, g2Facts, identifyTuiThread, parseG2Criteria, parseG2Transcript, readG2Criteria, sanitizeTranscript,
  splitCommandLine, stageClientCopy, unverifiedNames, defaultInjectText,
} from '../lib/g2.mjs';
import { sha256, parseSections } from '../lib/g1.mjs';
import { SCORES, ReportError, credentialShapedFields, evaluateG2, parseOperatorScores, schemaBlockFor, versionsVerified, writeRefusal, renderReport } from '../lib/g2-report.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..', '..');
const read = (p) => readFileSync(p, 'utf8');
const BASELINE = read(join(REPO, BASELINE_TRANSCRIPT));
const OLD_BASELINE = read(join(REPO, FIXTURE_DIR, 'transcript.jsonl'));
const PIN = parseCodexLastObserved(read(join(REPO, 'docs', 'planning', 'PINS.md'))).lastObserved;
const REPORT = join(REPO, 'tools', 'herdr', 'lib', 'g2-report.mjs');
const throws = (fn, cls, re) => {
  try {
    fn();
    return false;
  } catch (e) {
    return (!cls || e instanceof cls) && (!re || re.test(e.message));
  }
};

export function g2Unit(check) {
  // --- Codex pin-move check ------------------------------------------------------------------
  const pins = read(join(REPO, 'docs', 'planning', 'PINS.md'));
  const real = parseCodexLastObserved(pins);
  check('g2 pin: PINS.md "Codex CLI / app-server" last-observed version and commit parse', /^\d+\.\d+\.\d+$/.test(real.lastObserved) && /^[0-9a-f]{40}$/.test(real.commit ?? ''), JSON.stringify({ v: real.lastObserved, c: real.commit }));
  const table = (cell) => `| Surface | Stability label | Pinned version | Gates affected |\n|---|---|---|---|\n| Codex CLI / app-server | experimental | ${cell} | G2 |\n`;
  check('g2 pin: synthetic floating row -> version and commit', JSON.stringify(parseCodexLastObserved(table(`**floating** — last observed \`@scope/codex@0.160.2\` (commit \`${'a'.repeat(40)}\`); see policy`))) .includes('"lastObserved":"0.160.2"'));
  check('g2 pin: a row without a last-observed version throws', throws(() => parseCodexLastObserved(table('`0.154.0`'))));
  check('g2 pin: codex --version, userAgent and daemon version parse', parseCodexCliVersion('codex-cli 0.157.1\n') === '0.157.1' && parseCodexCliVersion('N/A (not runnable: ENOENT)') === null && parseCodexCliVersion('0.157.1') === null &&
    parseCodexUserAgentVersion('codex-tui/0.157.1 (Windows 10.0.26200; x86_64) unknown (oac_g2_spike; 0.0.1)') === '0.157.1' && parseCodexUserAgentVersion('oac_g2_spike/0.154.0 (Windows)') === '0.154.0' && parseCodexUserAgentVersion('no version here') === null);
  const dv = parseCodexDaemonVersion('{"status":"running","pid":4,"socketPath":"/x","managedCodexVersion":"0.157.1","cliVersion":"0.157.1","appServerVersion":"0.157.1"}\n');
  check('g2 pin: daemon version keeps only status and the three version fields', JSON.stringify(dv) === '{"status":"running","cliVersion":"0.157.1","appServerVersion":"0.157.1","managedCodexVersion":"0.157.1"}' && parseCodexDaemonVersion('not json') === null);
  const trig = codexPinMoveTrigger({ observed: '0.158.0', lastObserved: '0.157.1', source: '`codex --version`' });
  check('g2 pin: equal -> no trigger; different -> a pin-move trigger naming both, re-verify §3.2, no PINS.md edit', codexPinMoveTrigger({ observed: '0.157.1', lastObserved: '0.157.1', source: 'x' }) === null && /^PIN-MOVE TRIGGER/.test(trig) && trig.includes('0.158.0') && trig.includes('0.157.1') && /§3\.2/.test(trig) && /does not edit PINS\.md/.test(trig) && /no parseable version/.test(codexPinMoveTrigger({ observed: null, lastObserved: '0.157.1', source: 'x' })));

  // --- criteria, read from the reference file ---------------------------------------------
  const crit = readG2Criteria(REPO);
  check('g2: the four G2 criteria are read verbatim from the oac-gates reference', crit.length === 4 && /^A TUI launched normally \(no config overrides\) attaches/.test(crit[0]) && /control socket `CODEX_HOME\/app-server-control\/app-server-control\.sock`\)\.$/.test(crit[0]) && /^A second client of the same daemon/.test(crit[1]) && /^The TUI user sees/.test(crit[2]) && /^OAC holds no /.test(crit[3]));
  check('g2: a reference with the wrong number of criteria is refused', throws(() => parseG2Criteria('## Pass criteria\n\n- [ ] one\n\n## Next\n')));

  // --- facts from the REAL human-run fixtures -------------------------------------------------
  const bf = g2Facts(parseG2Transcript(BASELINE));
  check('g2 facts: 0.157.1 fixture -- four connections (list, turn, busyqueue, turns), all upgraded, userAgent 0.157.1', bf.connections.map((c) => c.mode).join() === 'list,turn,busyqueue,turns' && bf.connections.every((c) => c.upgraded && c.userAgentVersion === '0.157.1' && c.errors.length === 0));
  check('g2 facts: 0.157.1 fixture -- turn/start lines 16 and 24, thread/queue/add line 28, turn record line 40', bf.turnStarts.map((t) => t.reqLine).join() === '16,24' && bf.queueAdds[0].reqLine === 28 && bf.queueAdds[0].queuedId === '01a0dc63-2e68-7633-95c9-46ba70eaeec3' && bf.turnsLists[0].line === 40);
  const recTurns = bf.turnsLists[0].turns;
  check('g2 facts: the daemon record shows the queued turn carrying the queue/add clientUserMessageId, all completed, answers as G2-result.md', recTurns.every((t) => t.status === 'completed') && recTurns[0].userClientIds[0] === bf.queueAdds[0].clientUserMessageId && recTurns[0].agentMessages[0] === 'OAC G2 QUEUED' && recTurns[2].agentMessages[0] === 'OAC G2 RERUN RECEIVED');
  const bt = identifyTuiThread(bf, { operatorPrompt: DEFAULT_OPERATOR_PROMPT, projectDirs: ['<SPIKE_DIR>'] });
  check('g2 facts: the human run\'s TUI thread is identified by preview + cwd + loaded list, as G2-result.md did', bt.threadId === '01a0dc61-364b-7ed3-b686-b04e07eecaf2' && bt.candidates[0].listLine === 10);
  check('g2 facts: another prompt or another directory identifies nothing', !identifyTuiThread(bf, { operatorPrompt: 'Something else', projectDirs: ['<SPIKE_DIR>'] }).threadId && !identifyTuiThread(bf, { operatorPrompt: DEFAULT_OPERATOR_PROMPT, projectDirs: ['/elsewhere'] }).threadId);
  const of = g2Facts(parseG2Transcript(OLD_BASELINE));
  check('g2 facts: 0.154.0 fixture -- the watch connection\'s event stream: turn/started, agent message, turn/completed for injection 4', of.resumes.length === 1 && of.resumes[0].mode === 'watch' && of.events.turnCompleted.some((e) => e.mode === 'watch' && e.agentMessages.includes('OAC G2 EVENTS')) && of.events.turnStarted.length === 1 && of.connections[0].userAgentVersion === '0.154.0');
  check('g2 facts: 0.154.0 fixture -- two loaded threads at line 16 (the unidentified second thread)', of.loadedLists.find((l) => l.line === 16)?.data.length === 2);
  const self = compareByMode(parseG2Transcript(BASELINE), parseG2Transcript(BASELINE));
  check('g2 compare: the 0.157.1 fixture against itself is the same sequence in every mode', self.modes.length === 4 && self.modes.every((m) => m.same) && self.runOnlyModes.length === 0);
  const noQueue = BASELINE.split('\n').filter((l) => !/thread\/queue\/add|queuedSubmission/.test(l)).join('\n');
  const nq = compareByMode(parseG2Transcript(BASELINE), parseG2Transcript(noQueue));
  check('g2 compare: dropping thread/queue/add shows exactly the request and its result removed from busyqueue', nq.modes.find((m) => m.mode === 'busyqueue').ops.filter((o) => o.op === '-').map((o) => o.key).join('|') === 'c->d request thread/queue/add|d->c result (thread/queue/add)');

  // --- sanitizer ------------------------------------------------------------------------------
  const sb = sanitizeTranscript(BASELINE, { ownThreadId: bt.threadId });
  check('g2 sanitize: the committed (already redacted) 0.157.1 fixture passes through byte-identical, nothing to do', sb.text === BASELINE && sb.report.threadListEntriesRemoved === 0 && sb.report.installationIds === 0 && sb.report.serverNames === 0, JSON.stringify(sb.report));
  const L = (o) => JSON.stringify(o);
  const synth = [
    L({ t: 't1', mode: 'list', direction: 'handshake', payload: 'HTTP/1.1 101 Switching Protocols\r\nx: y' }),
    L({ t: 't2', mode: 'list', direction: 'client->daemon', payload: { jsonrpc: '2.0', id: 0, method: 'initialize', params: {} } }),
    L({ t: 't3', mode: 'list', direction: 'daemon->client', payload: { method: 'remoteControl/status/changed', params: { status: 'disabled', serverName: 'secret-host-9', installationId: 'inst-123-abc', environmentId: null } } }),
    L({ t: 't4', mode: 'list', direction: 'daemon->client', payload: { method: 'account/updated', params: { authMode: 'chatgpt', planType: 'pro' } } }),
    L({ t: 't5', mode: 'list', direction: 'client->daemon', payload: { jsonrpc: '2.0', id: 2, method: 'thread/list', params: {} } }),
    L({ t: 't6', mode: 'list', direction: 'daemon->client', payload: { id: 2, result: { data: [{ id: 'mine', preview: 'Greet me in a single word.' }, { id: 'other', preview: 'PRIVATE preview', path: '/x/rollout.jsonl' }] } } }),
    L({ t: 't7', mode: 'watch', direction: 'daemon->client', payload: { method: 'account/rateLimits/updated', params: { rateLimits: { credits: { balance: 3 }, planType: 'pro', primary: { usedPercent: 1 } } } } }),
  ].join('\n') + '\n';
  const ss = sanitizeTranscript(synth, { ownThreadId: 'mine' });
  check('g2 sanitize: unrelated sessions dropped and marked; host, installation id, plan and credits replaced; installation id handed back', !/PRIVATE|secret-host-9|inst-123-abc|"pro"|balance/.test(ss.text) && ss.text.includes('"id":"mine"') && ss.text.includes('unrelated sessions removed for privacy') && ss.report.threadListEntriesRemoved === 1 && ss.installationIds.join() === 'inst-123-abc' && ss.text.includes('"usedPercent":1'), ss.text);
  const sn = sanitizeTranscript(synth, { ownThreadId: null });
  check('g2 sanitize: with no identified thread, every thread/list entry is dropped (fail closed)', !sn.text.includes('"id":"mine"') && sn.report.threadListEntriesRemoved === 2);

  // --- pane classification and the argv proof ----------------------------------------------
  const trust = 'Do you trust the files in this folder?\n\n› 1. Yes, continue\n  2. No, quit\n\nPress enter to continue';
  const tc = classifyCodexScreen(trust);
  check('g2 pane: a trust dialog is recognized, option 1 selected, and the driver may accept it', tc.dialog === 'workspace-trust' && tc.selected?.number === 1 && driverMayAcceptCodex(tc).ok);
  check('g2 pane: with "No" preselected the driver refuses', !driverMayAcceptCodex(classifyCodexScreen(trust.replace('› 1.', '  1.').replace('  2. No', '› 2. No'))).ok);
  const unk = classifyCodexScreen('Allow command?\n› 1. Yes\n  2. No\nPress enter to confirm');
  check('g2 pane: an approval or any unnamed prompt is "unknown" and never driver-accepted', unk.dialog === 'unknown' && !driverMayAcceptCodex(unk).ok);
  check('g2 pane: in-progress indicator is a parameter; an idle composer is no dialog', classifyCodexScreen('• Working (3s • esc to interrupt)').busy && classifyCodexScreen('› Ask Codex to do anything').dialog === null && !classifyCodexScreen('› Ask Codex').busy);
  const npm = codexLaunchProof([{ pid: 7, argv: ['node', '/usr/lib/node_modules/@scope/codex/bin/codex.js'] }]);
  check('g2 argv: `node .../bin/codex.js` with nothing after it is a plain launch', npm.found && npm.plain && npm.pid === 7 && npm.codexToken === 'codex.js');
  const ov = codexLaunchProof([{ pid: 3, argv: ['bash'] }, { pid: 8, argv: ['/opt/codex/codex', '-c', 'model=x'] }]);
  check('g2 argv: arguments after codex (a config override) are not plain', ov.found && !ov.plain && JSON.stringify(ov.argsAfterCodex) === '["-c","model=x"]' && ov.pid === 8);
  const win = codexLaunchProof([{ pid: 9, argv: null, commandLine: '"C:\\Program Files\\nodejs\\node.exe" "C:\\Users\\x\\AppData\\Roaming\\npm\\node_modules\\@scope\\codex\\bin\\codex.js" --remote ws://127.0.0.1:1' }]);
  check('g2 argv: a Windows command line is split and checked the same way', win.found && !win.plain && win.argsAfterCodex[0] === '--remote' && splitCommandLine('"a b" c').join('|') === 'a b|c');
  check('g2 argv: no codex process -> not found (never assumed plain)', !codexLaunchProof([{ pid: 1, argv: ['bash', '-l'] }, { pid: 2, argv: null, commandLine: null }]).found);

  // --- staging, names, guards ---------------------------------------------------------------
  const tmp = mkdtempSync(join(tmpdir(), 'oac-g2-unit-'));
  try {
    const st = stageClientCopy(REPO, COMMITTED_CLIENT, tmp);
    check('g2: the client copy is staged from the blob at HEAD, sha256 = the one the G2 runs used', st.match && st.committedSha256 === COMMITTED_CLIENT_SHA256 && st.copySha256 === COMMITTED_CLIENT_SHA256 && st.copyPath === join(tmp, 'client.mjs') && st.workingTreeMatchesHead === true);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
  const scen = read(join(REPO, 'tools', 'herdr', 'scenarios', 'g2-codex-inject.mjs'));
  check('g2: the scenario never imports the quarantined client (static or dynamic import, require)', !/^\s*import\s[^;]*?from\s*['"][^'"]*(?:throwaway-quarantined|client\.mjs)['"]/m.test(scen) && !/\bimport\s*\(\s*[^)]*(?:throwaway-quarantined|client\.mjs)/.test(scen) && !/require\([^)]*client/.test(scen) && /\[join\(clientDir, 'client\.mjs'\), mode/.test(scen));
  check('g2: the default launch is plain `codex`', JSON.stringify(G2_LAUNCH) === '["codex"]');
  check('g2: the default operator prompt is not a delivered message; delivered texts are refused as operator input', !throws(() => assertNotInjected('p', DEFAULT_OPERATOR_PROMPT)) && throws(() => assertNotInjected('p', defaultInjectText('0.157.1'))) && throws(() => assertNotInjected('p', 'please call thread/queue/add')));
  check('g2: fixture names follow K7; unverified names are never fixture-shaped', JSON.stringify(fixtureNames('2026-10-01', '0.157.1')) === '{"transcript":"transcript-2026-10-01-0.157.1-herdr.jsonl","pane":"pane-2026-10-01-0.157.1-herdr.txt"}' && unverifiedNames('2026-10-01').transcript.startsWith('unverified-') && throws(() => fixtureNames('2026-10-01', 'latest')));

  // --- report rules ---------------------------------------------------------------------------
  check('g2 report: only criterion 3 takes an operator score, with a note', throws(() => parseOperatorScores([{ n: 1, score: 'equivalent', note: 'x' }]), ReportError, /mechanically/) && throws(() => parseOperatorScores([{ n: 3, score: 'equivalent', note: ' ' }]), ReportError, /note/) && parseOperatorScores([{ n: 3, score: 'not-equivalent', note: 'x' }])[3].score === SCORES.NEQ);
  const nr = evaluateG2({ manifest: { outcome: 'NOT RUN', outcomeReason: 'PIN-MOVE TRIGGER: x', scenarioData: {} }, transcriptText: null, paneText: null, baselineText: BASELINE, criteria: crit });
  check('g2 report: a NOT RUN leaves every criterion not evaluable', nr.rows.length === 4 && nr.rows.every((r) => r.score === SCORES.NE && /NOT RUN/.test(r.reason)));
  const V = (o = {}, post = true) => ({ versions: { verified: true, cli: '0.157.1', wire: '0.157.1', daemon: { cliVersion: '0.157.1', appServerVersion: '0.157.1', managedCodexVersion: '0.157.1' }, pinsLastObserved: '0.157.1', pinsSource: { workingTreeMatchesHead: true }, ...o }, postRun: { matches: post } });
  check('g2 report: versions verified only when CLI, all daemon fields and the wire equal the pin, before and after', versionsVerified(V()) && !versionsVerified(V({ wire: '0.158.0' })) && !versionsVerified(V({ daemon: { cliVersion: '0.157.1', appServerVersion: '0.158.0', managedCodexVersion: '0.157.1' } })) && !versionsVerified(V({}, false)) && !versionsVerified(V({ pinsSource: { workingTreeMatchesHead: false } })));
  const okRun = { outcome: 'PASS', driver: { commit: 'a'.repeat(40), toolsHerdrDirty: false }, captures: [{ file: 'transcript-x-herdr.jsonl', written: true }, { file: 'pane-x-herdr.txt', written: true }], scenarioData: { g2: { ...V(), fixtures: { transcript: 'transcript-x-herdr.jsonl', pane: 'pane-x-herdr.txt' }, captureNames: { transcript: 'transcript-x-herdr.jsonl', pane: 'pane-x-herdr.txt' }, client: { match: true, workingTreeMatchesHead: true } } } };
  check('g2 report: --write accepts only a verified, clean-driver PASS; a dirty tools/herdr/ is refused (unlike G1\'s generator)', writeRefusal(okRun) === null && /toolsHerdrDirty true/.test(writeRefusal({ ...okRun, driver: { commit: 'a'.repeat(40), toolsHerdrDirty: true } })) && /only a PASS run/.test(writeRefusal({ ...okRun, outcome: 'FAIL' })) && /not verified/.test(writeRefusal({ ...okRun, scenarioData: { g2: { ...okRun.scenarioData.g2, ...V({}, false) } } })));
  const manifestAtHead = JSON.parse(spawnSync('git', ['show', `HEAD:${MANIFEST_PATH}`], { cwd: REPO, encoding: 'utf8', maxBuffer: 64 << 20 }).stdout);
  const sch = schemaBlockFor({ version: '0.157.1', codexCommit: 'c', manifestJson: manifestAtHead });
  const sch2 = schemaBlockFor({ version: '0.158.0', codexCommit: 'deadbeef', manifestJson: manifestAtHead });
  check('g2 report: schema block for 0.157.1 reuses MANIFEST.json\'s regenerated-schema record and says the capture is NOT yet validated', sch.upstream?.tree_sha256 && sch.local_generation?.cli === 'codex-cli 0.157.1' && /^NOT yet validated/.test(sch.validated_against));
  check('g2 report: for a version with no schema record, the not-regenerated shape with no hash', sch2.upstream === null && sch2.local_generation === null && sch2.sha256 === null && sch2.commit === 'deadbeef' && /not regenerated/.test(sch2.note));
  check('g2 report: credential-shaped fields are found; the human run\'s client frames carry none', credentialShapedFields({ params: { apiKey: 'x', nested: [{ note: 'Bearer abcdef' }] } }).length === 2 && parseG2Transcript(BASELINE).filter((e) => e.direction === 'client->daemon').every((e) => credentialShapedFields(e.payload).length === 0));
  const tpl = renderReport({ manifest: { outcome: 'NOT RUN', scenarioData: { g2: {} } }, evaluation: nr, diffText: null, date: '2026-10-01', fixtures: null, runManifestName: 'x' });
  check('g2 report: the record carries the operator attestation UNTICKED and no equivalence callout', /^## Operator attestation$/m.test(tpl) && (tpl.match(/^- \[ \] \*\*(?:herdr|Harness|Consent dialog):\*\*/gm) ?? []).length === 3 && !/^- \[x\]/m.test(tpl) && /^- \*\*Attested by:\*\* <operator>, <YYYY-MM-DD>$/m.test(tpl) && !/Equivalence record\*\* for G/.test(tpl) && /Not verdict-bearing/.test(tpl));
}

// --- lifecycle cases (driver end to end against the fakes) --------------------------------

const today = () => new Date().toISOString().slice(0, 10);
const FAST = ['--param', 'settleMs=300', '--param', 'pollMs=200', '--param', 'listPollMs=500', '--param', 'wireTimeoutMs=10000', '--param', 'turnTimeoutMs=30000', '--param', 'startupTimeoutMs=20000', '--param', 'attachTimeoutMs=15000', '--param', 'busyQueueTimeoutMs=60000'];
const TRACER = join(HERE, 'fs-trace.mjs');

// A fake `codex` on PATH: a copy of test/fake-codex.mjs named `codex`, so a pane process's
// argv reads `node <base>/bin/codex` like an npm-installed Codex. With trace: every Node
// process the driver starts is traced (test/fs-trace.mjs) into <base>/fs-trace.jsonl.
export function fakeCodexEnv(base, { trace = false, ...env } = {}) {
  const bin = join(base, 'bin');
  mkdirSync(bin, { recursive: true });
  const p = join(bin, 'codex');
  copyFileSync(join(HERE, 'fake-codex.mjs'), p);
  chmodSync(p, 0o755);
  return {
    FAKE_CODEX_VERSION: PIN,
    ...env,
    PATH: `${bin}:${process.env.PATH}`,
    ...(trace ? { NODE_OPTIONS: `--import=${pathToFileURL(TRACER).href}`, OAC_FS_TRACE_FILE: join(base, 'fs-trace.jsonl') } : {}),
  };
}

// The fake daemon runs outside the herdr session (like the real one, which the driver never
// stops); the self-test stops it after each case.
export function stopFakeCodexDaemon(codexHome) {
  const f = codexHome && join(codexHome, 'app-server-control', 'fake-daemon.pid');
  if (!f || !existsSync(f)) return;
  try {
    process.kill(Number(read(f)), 'SIGKILL');
  } catch {
    /* gone */
  }
}

const inside = (p, root) => {
  const rel = relative(root, p);
  return rel === '' || (!rel.startsWith('..') && !isAbsolute(rel));
};

export function g2Cases(check) {
  const cases = [];
  const run = (name, opts, assert) => cases.push({ name, opts: { scenario: 'g2-codex-inject', mode: 'fake-codex', ...opts }, assert });
  const names = () => fixtureNames(today(), PIN);
  const crit = readG2Criteria(REPO);
  const evalRun = (r, operatorScores = {}) => evaluateG2({ manifest: r.manifest, transcriptText: r.capture(names().transcript), paneText: r.capture(names().pane), baselineText: BASELINE, criteria: crit, operatorScores });
  const daemonStarted = (r) => existsSync(join(r.env.CODEX_HOME, 'app-server-control', 'fake-daemon.pid'));
  const DELIVERED = /OAC G2|second daemon client|G2 spike (?:busy-turn|queued)/;

  run('g2 human accept (traced)', { args: ['--param', 'accept=human', ...FAST], fakeCodex: { FAKE_CODEX_SELF_ACCEPT_MS: '1500', trace: true } }, (r) => {
    const m = r.manifest;
    const g2 = m.scenarioData.g2;
    check('g2 human: PASS (exit 0)', r.status === 0 && m.outcome === 'PASS', `${r.status} ${m.outcome} ${m.outcomeReason}`);
    check('g2 human: plain `codex` launched through agent start --kind codex with nothing after it', JSON.stringify(m.launch.argv) === '["codex"]' && g2.launch.verbatim && r.calls.some((c) => /agent start g2codex --kind codex --pane w1:p1 --timeout \d+$/.test(c.argv.join(' '))));
    const pa = g2.paneArgv[0];
    check('g2 human: the pane process argv, read from /proc, is `node <base>/bin/codex` with no argument after codex', pa.proof.found && pa.proof.plain && pa.argv.some((a) => a.source.startsWith('/proc/') && a.argv?.length === 2 && basename(a.argv[1]) === 'codex'), JSON.stringify(pa));
    check('g2 human: `codex app-server daemon start` ran first, then daemon version; CLI, daemon and wire all verified', g2.daemon.start.exitCode === 0 && g2.versions.verified && g2.versions.daemon.appServerVersion === PIN && g2.versions.wire === PIN && g2.postRun.matches && daemonStarted(r));
    check('g2 human: client staged from HEAD, sha256 matches the committed blob; ran unmodified in every mode', g2.client.match && g2.client.copySha256 === COMMITTED_CLIENT_SHA256 && g2.client.copy === '<SCRATCH>/g2-client/client.mjs' && g2.clientRuns.map((x) => x.mode).join() === 'list,list,turn,busyqueue,turns' && g2.clientRuns.every((x) => x.problems.length === 0 && x.exitCode === 0) && g2.divergence.length === 0, JSON.stringify(g2.clientRuns.map((x) => [x.mode, x.exitCode, x.problems])));
    const d = g2.dialogs[0];
    check('g2 human: the trust dialog was read verbatim and accepted outside the driver (no keystroke sent)', d?.kind === 'workspace-trust' && d.acceptOrigin === 'human' && d.inputBetweenReadAndAccept === 0 && !m.commands.some((x) => x.role === 'dialog-accept' || x.argv.includes('send-keys')));
    check('g2 human: herdr typed exactly one prompt, the operator\'s own message; no delivered text ever went through herdr', r.prompts.length === 1 && r.prompts[0].text === DEFAULT_OPERATOR_PROMPT && m.commands.every((x) => !x.argv.some((a) => DELIVERED.test(a))));
    check('g2 human: each delivery sent once', g2.injectionsSent.length === 2);
    const cap = r.capture(names().transcript);
    const f = g2Facts(parseG2Transcript(cap));
    check('g2 human: the wire shows turn/start (turn), turn/start + thread/queue/add (busyqueue), completion on the watch event stream', f.turnStarts.filter((t) => t.mode === 'turn').length === 1 && f.turnStarts.filter((t) => t.mode === 'busyqueue').length === 1 && f.queueAdds.length === 1 && f.events.turnCompleted.filter((e) => e.mode === 'watch').length === 3 && g2.idle.via === 'thread/resume result');
    check('g2 human: captures carry the K7 fixture names, written clean', m.captures.map((x) => x.file).sort().join() === [names().pane, names().transcript].sort().join() && m.captures.every((x) => x.written && x.redaction.residualLeaks.length === 0 && x.redaction.residualGenericHits.length === 0));
    check('g2 human: sanitized -- no unrelated session, host name, installation id or plan in the transcript', !/PRIVATE|fakehost-q7x|fa4e1d00|"plus"/.test(cap) && cap.includes('unrelated sessions removed for privacy') && g2.sanitizer.threadListEntriesRemoved > 0 && !r.manifestText.includes('fa4e1d00'));
    const ev = evalRun(r);
    check('g2 human: report scores C1, C2, C4 equivalent; C3 pending the operator', ev.rows.map((x) => x.score).join('|') === [SCORES.EQ, SCORES.EQ, SCORES.NE, SCORES.EQ].join('|') && /operator review/.test(ev.rows[2].reason), JSON.stringify(ev.rows.map((x) => [x.score, x.reason])));
    check('g2 human: an operator score for C3 applies once its preconditions hold', evalRun(r, parseOperatorScores([{ n: 3, score: 'equivalent', note: 'pane read shows both messages and answers' }])).rows[2].score === SCORES.EQ);
    const pane = parseSections(r.capture(names().pane));
    check('g2 human: the pane capture holds the dialog read and the post-delivery reads, keyed to herdr commands', pane.some((s) => s.seq === d.readSeq && /trust the files/.test(s.text)) && pane.some((s) => s.seq === g2.inject.afterReadSeq) && pane.some((s) => s.seq === g2.busyQueue.afterReadSeq));

    // Credential hygiene, from the trace rather than asserted: the driver and the client.
    const trace = read(join(r.base, 'fs-trace.jsonl')).split('\n').filter(Boolean).map((l) => JSON.parse(l));
    const home = realpathSync(r.env.CODEX_HOME);
    const RUN_JS = join(REPO, 'tools', 'herdr', 'run.mjs');
    const driver = trace.filter((t) => t.script && resolve(t.script) === RUN_JS);
    const client = trace.filter((t) => t.script && /[\\/]g2-client[\\/]client\.mjs$/.test(t.script));
    const underHome = (list) => list.filter((t) => t.kind === 'fs' && [t.path, (() => { try { return realpathSync(t.path); } catch { return t.path; } })()].some((p) => inside(p, home) || inside(p, r.env.CODEX_HOME)));
    const hashed = new Set(['config.toml', 'hooks.json'].map((n) => join(home, n)).concat(['config.toml', 'hooks.json'].map((n) => join(r.env.CODEX_HOME, n))));
    const driverHome = underHome(driver);
    check('g2 trace: the tracer saw the driver and the client (non-empty traces)', driver.length > 50 && client.length > 0, `${driver.length} ${client.length}`);
    check('g2 trace: positive control -- the driver\'s own harness-config hash reads under the Codex home ARE traced', driverHome.some((t) => t.path.endsWith('config.toml')) && driverHome.some((t) => t.path.endsWith('hooks.json')));
    check('g2 trace: the driver opened nothing else under the Codex home (no credential file, no sessions, no socket)', driverHome.every((t) => hashed.has(t.path)), JSON.stringify([...new Set(driverHome.filter((t) => !hashed.has(t.path)).map((t) => t.path))]));
    check('g2 trace: the staged client opened nothing under the Codex home at all', underHome(client).length === 0, JSON.stringify(underHome(client).map((t) => t.path)));
    const exe = (t) => basename(t.file).replace(/\.exe$/i, '');
    const driverExes = [...new Set(driver.filter((t) => t.kind === 'spawn').map(exe))].sort();
    const clientExes = [...new Set(client.filter((t) => t.kind === 'spawn').map(exe))].sort();
    check('g2 trace: the driver started only node (herdr, the client), git and codex -- no credential-store tool', driverExes.every((e) => [basename(process.execPath), 'git', 'codex'].includes(e)), driverExes.join(','));
    check('g2 trace: the client started only `codex app-server proxy`', clientExes.join() === 'codex' && client.filter((t) => t.kind === 'spawn').every((t) => JSON.stringify(t.args) === '["app-server","proxy"]'), JSON.stringify(client.filter((t) => t.kind === 'spawn')));

    // The report CLI: draft, then --write into a temporary root (never the repo).
    const draft = spawnSync(process.execPath, [REPORT, '--run', r.outDir], { encoding: 'utf8', timeout: 20000 });
    check('g2 report CLI: draft printed, not verdict-bearing, attestation unticked, with the per-connection diff', draft.status === 0 && /Not verdict-bearing/.test(draft.stdout) && /## Method-sequence diff/.test(draft.stdout) && /^- \[ \] \*\*herdr:\*\*/m.test(draft.stdout) && /== busyqueue:/.test(draft.stdout), draft.stderr);
    const bad = spawnSync(process.execPath, [REPORT, '--run', r.outDir, '--score', '4=equivalent', '--note', '4=trust me'], { encoding: 'utf8', timeout: 20000 });
    check('g2 report CLI: refuses an operator score for a mechanically scored criterion', bad.status === 2 && /only criterion 3 takes an operator score/.test(bad.stderr));
    const root = mkdtempSync(join(tmpdir(), 'oac-g2-report-'));
    try {
      const w = spawnSync(process.execPath, [REPORT, '--run', r.outDir, '--write', '--root', root], { encoding: 'utf8', timeout: 20000 });
      if (m.driver.toolsHerdrDirty === false) {
        const date = g2.date;
        const expected = [`docs/planning/gates/herdr-runs/G2-${date}.md`, `docs/planning/gates/herdr-runs/G2-${date}.run-manifest.json`, `${FIXTURE_DIR}/${names().transcript}`, `${FIXTURE_DIR}/${names().pane}`];
        check('g2 report CLI --write (clean tools/herdr/): report, run manifest and both fixtures written under the root', w.status === 0 && expected.every((p) => existsSync(join(root, p))), w.stdout + w.stderr);
        const entries = JSON.parse(read(join(r.outDir, 'manifest-entries.draft.json')));
        const REQUIRED = ['path', 'provider', 'surface', 'observed_version', 'pins_row', 'pins_as_of', 'version_matches_pin', 'capture_date', 'capture_utc_range', 'superseded_by', 'redaction', 'coverage'];
        check('g2 report CLI --write: draft entries carry every required field, the driver block and a schema block', entries.length === 2 && entries.every((e) => REQUIRED.every((k) => k in e) && e.provider === 'codex' && e.version_matches_pin === true && e.driver.herdr_version === 'herdr 0.9.1' && /^[0-9a-f]{40}$/.test(e.driver.driver_commit) && e.driver.run_manifest === `docs/planning/gates/herdr-runs/G2-${date}.run-manifest.json` && e.schema) && entries[0].schema.local_generation?.cli === `codex-cli ${PIN}` && /^NOT yet validated/.test(entries[0].schema.validated_against) && entries[1].schema.applicable === false);
        const again = spawnSync(process.execPath, [REPORT, '--run', r.outDir, '--write', '--root', root], { encoding: 'utf8', timeout: 20000 });
        check('g2 report CLI --write: never overwrites', again.status === 2 && /refusing to overwrite/.test(again.stderr));
      } else {
        check('g2 report CLI --write (tools/herdr/ dirty in this checkout): refused, nothing written', w.status === 2 && /toolsHerdrDirty/.test(w.stderr) && readdirSync(root).length === 0 && !existsSync(join(r.outDir, 'manifest-entries.draft.json')), w.stderr);
      }
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  run('g2 driver accept, Codex settles to unknown', { args: ['--param', 'accept=driver', ...FAST], fakeCodex: { FAKE_CODEX_POST_STATE: 'unknown' } }, (r) => {
    const m = r.manifest;
    const g2 = m.scenarioData.g2;
    check('g2 unknown: PASS', r.status === 0 && m.outcome === 'PASS', `${m.outcome} ${m.outcomeReason}`);
    check('g2 unknown: herdr\'s `unknown` state was observed and recorded, with a finding', g2.herdrStates.some((s) => s.state === 'unknown') && m.findings.some((f) => /`unknown`/.test(f) && /nothing re-sent/.test(f)), JSON.stringify(g2.herdrStates));
    check('g2 unknown: it triggered no re-submission -- one operator prompt, each delivery once on the wire', r.prompts.length === 1 && g2.injectionsSent.length === 2 && g2Facts(parseG2Transcript(r.capture(names().transcript))).turnStarts.length === 2);
    const d = g2.dialogs[0];
    const acc = m.commands.find((x) => x.role === 'dialog-accept');
    check('g2 unknown: the trust dialog was read, then accepted by the driver with no input in between', d.acceptOrigin === 'driver' && acc?.seq === d.acceptSeq && m.commands.find((x) => x.seq === acc.seq - 1)?.argv.includes('read') && d.inputBetweenReadAndAccept === 0);
  });

  run('g2 CLI version is a pin-move trigger', { args: FAST, fakeCodex: { FAKE_CODEX_CLI_VERSION: '0.158.0' } }, (r) => {
    const m = r.manifest;
    check('g2 pin move (CLI): NOT RUN with a pin-move trigger naming both versions', r.status === 3 && /^PIN-MOVE TRIGGER: `codex --version` reports 0\.158\.0/.test(m.outcomeReason) && m.outcomeReason.includes(PIN) && m.findings.some((f) => /^PIN-MOVE TRIGGER/.test(f)), m.outcomeReason);
    check('g2 pin move (CLI): nothing started -- no daemon, no workspace, no agent, no capture', !daemonStarted(r) && !r.calls.some((c) => c.argv.includes('workspace') || c.argv.includes('agent')) && m.captures.length === 0);
  });

  run('g2 daemon version is a pin-move trigger', { args: FAST, fakeCodex: { FAKE_CODEX_DAEMON_VERSION: '0.158.0' } }, (r) => {
    const m = r.manifest;
    check('g2 pin move (daemon): NOT RUN, the trigger names the daemon field', r.status === 3 && /^PIN-MOVE TRIGGER: `codex app-server daemon version` cliVersion reports 0\.158\.0/.test(m.outcomeReason), m.outcomeReason);
    check('g2 pin move (daemon): no launch, no client run, no fixture-named capture', !r.calls.some((c) => c.argv.includes('agent')) && m.scenarioData.g2.clientRuns.length === 0 && m.scenarioData.g2.fixtures === null && m.captures.every((c) => c.file.startsWith('unverified-')));
  });

  run('g2 wire version is a pin-move trigger', { args: FAST, fakeCodex: { FAKE_CODEX_WIRE_VERSION: '0.158.0' } }, (r) => {
    const m = r.manifest;
    const g2 = m.scenarioData.g2;
    check('g2 pin move (wire): NOT RUN with a pin-move trigger from the initialize userAgent', r.status === 3 && /^PIN-MOVE TRIGGER: the wire initialize userAgent reports 0\.158\.0/.test(m.outcomeReason), m.outcomeReason);
    check('g2 pin move (wire): TUI never launched, nothing delivered; captures unverified-* only', !r.calls.some((c) => c.argv.includes('agent')) && g2.injectionsSent.length === 0 && g2.fixtures === null && g2.versions.verified === false && m.captures.length > 0 && m.captures.every((c) => c.file.startsWith('unverified-')), JSON.stringify(m.captures.map((c) => c.file)));
    const root = mkdtempSync(join(tmpdir(), 'oac-g2-report-'));
    try {
      const w = spawnSync(process.execPath, [REPORT, '--run', r.outDir, '--write', '--root', root], { encoding: 'utf8', timeout: 20000 });
      const p = spawnSync(process.execPath, [REPORT, '--run', r.outDir], { encoding: 'utf8', timeout: 20000 });
      check('g2 pin move (wire): report --write refuses and writes nothing; the draft says NOT RUN, every criterion not evaluable', w.status === 2 && /--write refused: run outcome is NOT RUN/.test(w.stderr) && readdirSync(root).length === 0 && p.status === 0 && /Run outcome:\*\* NOT RUN/.test(p.stdout) && (p.stdout.match(/\*\*not evaluable\*\* \|/g) ?? []).length === 4, w.stderr + p.stdout.slice(0, 1500));
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  run('g2 launch with a config override', { args: ['--launch', '["codex","-c","model=other"]', ...FAST], fakeCodex: {} }, (r) => {
    check('g2 override: FAIL before anything starts', r.status === 1 && /is not plain `codex`/.test(r.manifest.outcomeReason) && !r.calls.some((c) => c.argv.includes('agent')) && !daemonStarted(r), r.manifest.outcomeReason);
  });

  run('g2 TUI does not attach (embedded server)', { args: ['--param', 'accept=driver', ...FAST, '--param', 'attachTimeoutMs=3000'], fakeCodex: { FAKE_CODEX_NO_ATTACH: '1' } }, (r) => {
    const m = r.manifest;
    const g2 = m.scenarioData.g2;
    check('g2 no attach: NOT RUN, recorded as a criterion-1 finding, nothing delivered', r.status === 3 && /no loaded thread matched/.test(m.outcomeReason) && m.findings.some((f) => /may not have attached/.test(f)) && g2.injectionsSent.length === 0 && g2.thread === null, m.outcomeReason);
    check('g2 no attach: the transcript holds only list runs, and the thread/list entries were all dropped (no thread identified)', g2.clientRuns.every((x) => x.mode === 'list') && !/PRIVATE/.test(r.capture(names().transcript)));
  });

  run('g2 client divergence', { args: ['--param', 'accept=driver', ...FAST], fakeCodex: { FAKE_CODEX_REJECT: 'thread/queue/add' } }, (r) => {
    const m = r.manifest;
    const g2 = m.scenarioData.g2;
    check('g2 divergence: FAIL, recorded as a divergence and a finding, the client not patched', r.status === 1 && /did not work unmodified/.test(m.outcomeReason) && g2.divergence.length === 1 && /thread\/queue\/add/.test(g2.divergence[0]) && m.findings.some((f) => /divergence/.test(f)), m.outcomeReason);
    check('g2 divergence: thread/queue/add sent once only, never retried', g2Facts(parseG2Transcript(r.capture(names().transcript))).queueAdds.length === 1);
    const ev = evalRun(r);
    check('g2 divergence: every criterion not evaluable', ev.rows.every((x) => x.score === SCORES.NE));
  });

  run('g2 human accept timeout', { args: ['--param', 'accept=human', '--param', 'humanAcceptTimeoutMs=1500', ...FAST], fakeCodex: {} }, (r) => {
    const m = r.manifest;
    const d = m.scenarioData.g2.dialogs[0];
    check('g2 human timeout: NOT RUN, nothing sent to the dialog, nothing delivered', r.status === 3 && /not accepted by the operator/.test(m.outcomeReason) && !m.commands.some((x) => x.seq > d.readSeq && ['operator-input', 'dialog-accept'].includes(x.role)) && m.scenarioData.g2.injectionsSent.length === 0, m.outcomeReason);
  });
  return cases;
}
