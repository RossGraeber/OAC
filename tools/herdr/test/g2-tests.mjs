// K7 (#130) tests, run by the driver self-test (`node tools/herdr/run.mjs --self-test`).
//
// Unit half: the Codex version warning (lib/pins.mjs, #216), lib/g2.mjs against the REAL committed
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
// about herdr or Codex: a live G2 run through herdr is UNVERIFIED until it runs live.

import { spawnSync } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, relative, resolve, isAbsolute } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { parseCodexVersions, parseCodexCliVersion, parseCodexUserAgentVersion, parseCodexDaemonVersion, codexVersionWarning, pinsReadWarning } from '../lib/pins.mjs';
import {
  BASELINE_TRANSCRIPT, COMMITTED_CLIENT, COMMITTED_CLIENT_SHA256, FIXTURE_DIR, G2_LAUNCH, MANIFEST_PATH, DEFAULT_OPERATOR_PROMPT, assertNotInjected, classifyCodexScreen,
  codexLaunchProof, compareByMode, driverMayAcceptCodex, fixtureNames, g2Facts, identifyTuiThread, parseG2Criteria, parseG2Transcript, readG2Criteria, sanitizeTranscript,
  splitCommandLine, splitWindowsCommandLine, stageClientCopy, unverifiedNames, defaultInjectText, G2_CRITERIA_SHA256, CriteriaDriftError, codexReadiness, waitCodexReady, loadedSince, codexReadyTimeoutFinding, multipleNewThreadsFinding,
  processArgv, minimizeArgv, paneArgv, argPlaceholder, arg0Placeholder, EXPECTED_EXECUTABLE,
} from '../lib/g2.mjs';
import { createRedactor, reportIsClean } from '../lib/redact.mjs';
import { sha256, parseSections } from '../lib/g1.mjs';
import { SCORES, ReportError, credentialShapedFields, evaluateG2, parseOperatorScores, schemaBlockFor, versionsVerified, versionMatchesLastTested, writeRefusal, fixtureWithheld, renderReport } from '../lib/g2-report.mjs';
import { cloneWithPins } from './g1-tests.mjs';
import { SYNTH } from './elide-tests.mjs';
import { parseWin32ProcessJson, parsePsTable } from '../lib/proc.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..', '..');
const read = (p) => readFileSync(p, 'utf8');
// Every regular file under dir, recursively (#232: what a run wrote).
const filesUnder = (dir) => (existsSync(dir) ? readdirSync(dir, { withFileTypes: true }).flatMap((d) => (d.isDirectory() ? filesUnder(join(dir, d.name)) : d.isFile() ? [join(dir, d.name)] : [])) : []);
// LF, as committed: a CRLF checkout (core.autocrlf=true) must not make the byte-identity
// checks below fail.
const BASELINE = read(join(REPO, BASELINE_TRANSCRIPT)).replace(/\r\n/g, '\n');
const OLD_BASELINE = read(join(REPO, FIXTURE_DIR, 'transcript.jsonl'));
// PINS.md's last tested Codex version: the fake Codex reports it unless a case sets another.
const PIN = parseCodexVersions(read(join(REPO, 'docs', 'planning', 'PINS.md'))).lastTested;
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
  // --- Codex version warning (#216: warn, never gate) -----------------------------------------
  const pins = read(join(REPO, 'docs', 'planning', 'PINS.md'));
  const real = parseCodexVersions(pins);
  check('g2 version: PINS.md "Codex CLI / app-server" minimum (the first worked-with 0.154.0), last tested version and its commit parse', real.minimum === '0.154.0' && /^\d+\.\d+\.\d+$/.test(real.lastTested) && /^[0-9a-f]{40}$/.test(real.commit ?? ''), JSON.stringify(real));
  const table = (cell) => `| Surface | Stability label | Pinned version | Gates affected |\n|---|---|---|---|\n| Codex CLI / app-server | experimental | ${cell} | G2 |\n`;
  const syn = parseCodexVersions(table(`**floating** — minimum \`@scope/codex@0.150.0\` (commit \`${'b'.repeat(40)}\`); last tested \`@scope/codex@0.160.2\` (commit \`${'a'.repeat(40)}\`); see policy`));
  check('g2 version: synthetic row -> minimum, last tested, and the last tested version\'s commit (not the minimum\'s)', syn.minimum === '0.150.0' && syn.lastTested === '0.160.2' && syn.commit === 'a'.repeat(40), JSON.stringify(syn));
  const badRows = [table('`0.154.0`'), table('**floating** — last observed `@scope/codex@0.157.1`'), table('**floating** — last tested `@scope/codex@0.157.1`'), 'nothing', undefined].map((t) => { try { return parseCodexVersions(t); } catch (e) { return { threw: e.message }; } });
  check('g2 version (#216 review): a malformed or missing row never throws; unreadable versions are null, the problem named, and pinsReadWarning reports it', badRows.every((p) => !p.threw && typeof p.problem === 'string') && badRows[2].lastTested === '0.157.1' && badRows[2].minimum === null && badRows[0].commit === null && /^VERSION WARNING \(G2\): could not read/.test(pinsReadWarning(badRows[3], 'G2')) && real.problem === null, JSON.stringify(badRows));
  check('g2 version (#216 review): null last tested/minimum -> a "could not be read" warning', /last tested version could not be read/.test(codexVersionWarning({ observed: '0.159.3', lastTested: null, minimum: null, source: 'x' })) && !/undefined|null/.test(codexVersionWarning({ observed: '0.159.3', lastTested: null, minimum: null, source: 'x' })));
  check('g2 pin: codex --version, userAgent and daemon version parse', parseCodexCliVersion('codex-cli 0.157.1\n') === '0.157.1' && parseCodexCliVersion('N/A (not runnable: ENOENT)') === null && parseCodexCliVersion('0.157.1') === null &&
    parseCodexUserAgentVersion('codex-tui/0.157.1 (Windows 10.0.26200; x86_64) unknown (oac_g2_spike; 0.0.1)') === '0.157.1' && parseCodexUserAgentVersion('oac_g2_spike/0.154.0 (Windows)') === '0.154.0' && parseCodexUserAgentVersion('no version here') === null);
  const dv = parseCodexDaemonVersion('{"status":"running","pid":4,"socketPath":"/x","managedCodexVersion":"0.157.1","cliVersion":"0.157.1","appServerVersion":"0.157.1"}\n');
  check('g2 pin: daemon version keeps only status and the three version fields', JSON.stringify(dv) === '{"status":"running","cliVersion":"0.157.1","appServerVersion":"0.157.1","managedCodexVersion":"0.157.1"}' && parseCodexDaemonVersion('not json') === null);
  const warn = codexVersionWarning({ observed: '0.160.0', lastTested: '0.159.3', minimum: '0.154.0', source: '`codex --version`' });
  check('g2 version: equal -> no warning; different -> a VERSION WARNING naming both, the run continues, no PINS.md edit', codexVersionWarning({ observed: '0.159.3', lastTested: '0.159.3', minimum: '0.154.0', source: 'x' }) === null && /^VERSION WARNING \(G2\)/.test(warn) && warn.includes('0.160.0') && warn.includes('0.159.3') && /run continues/.test(warn) && /does not edit PINS\.md/.test(warn) && !/PIN-MOVE|NOT RUN/.test(warn) && /no parseable version/.test(codexVersionWarning({ observed: null, lastTested: '0.159.3', minimum: '0.154.0', source: 'x' })), warn);
  check('g2 version: below the minimum is a warning that says so', /below the minimum 0\.154\.0/.test(codexVersionWarning({ observed: '0.153.9', lastTested: '0.159.3', minimum: '0.154.0', source: 'x' })));

  // --- criteria, read from the reference file ---------------------------------------------
  const { criteria: crit, reference: critRef } = readG2Criteria(REPO);
  check('g2: the four G2 criteria are read verbatim from the oac-gates reference', crit.length === 4 && /^A TUI launched normally \(no config overrides\) attaches/.test(crit[0]) && /control socket `CODEX_HOME\/app-server-control\/app-server-control\.sock`\)\.$/.test(crit[0]) && /^A second client of the same daemon/.test(crit[1]) && /^The TUI user sees/.test(crit[2]) && /^OAC holds no /.test(crit[3]));
  check('g2: a reference with the wrong number of criteria is refused', throws(() => parseG2Criteria('## Pass criteria\n\n- [ ] one\n\n## Next\n')));
  check('g2: the criteria are read from HEAD and match the pinned sha256; the source is recorded', critRef.criteriaSha256 === G2_CRITERIA_SHA256 && critRef.path === '.claude/skills/oac-gates/references/G2-codex-inject.md' && /^[0-9a-f]{40}$/.test(critRef.headCommit) && /^[0-9a-f]{64}$/.test(critRef.fileSha256));
  // Review finding (K7): scoring must not drift from the criteria text. A throwaway git repo
  // holds the reference; a committed reword or reorder is refused, an uncommitted edit is
  // never read.
  const refText = read(join(REPO, critRef.path));
  const lines = refText.split('\n');
  const idx = lines.map((l, i) => (/^- \[[ x]\] /.test(l) ? i : -1)).filter((i) => i !== -1);
  const gr = mkdtempSync(join(tmpdir(), 'oac-g2-crit-'));
  try {
    const git = (...a) => spawnSync('git', ['-c', 'user.email=t@example.invalid', '-c', 'user.name=t', '-c', 'commit.gpgsign=false', ...a], { cwd: gr, encoding: 'utf8', timeout: 15000 });
    const put = (text) => {
      mkdirSync(join(gr, dirname(critRef.path)), { recursive: true });
      writeFileSync(join(gr, critRef.path), text);
    };
    git('init', '-q');
    put(refText);
    git('add', '.');
    git('commit', '-q', '-m', 'ref');
    const same = readG2Criteria(gr);
    const reworded = lines.map((l, i) => (i === idx[3] ? l.replace(/at any point in the exchange/, 'most of the time') : l)).join('\n');
    put(reworded);
    const uncommitted = readG2Criteria(gr);
    const swap = [...lines];
    [swap[idx[2]], swap[idx[3]]] = [swap[idx[3]], swap[idx[2]]];
    const drift = (text) => {
      put(text);
      git('commit', '-q', '-am', 'drift');
      return throws(() => readG2Criteria(gr), CriteriaDriftError, /the reference changed; re-review K7's scoring/);
    };
    check('g2 criteria drift: the committed reference as K7 found it passes', same.reference.criteriaSha256 === G2_CRITERIA_SHA256 && same.reference.workingTreeMatchesHead);
    check('g2 criteria drift: an UNCOMMITTED reword is never read (HEAD is), and is reported', reworded !== refText && JSON.stringify(uncommitted.criteria) === JSON.stringify(crit) && uncommitted.reference.workingTreeMatchesHead === false);
    check('g2 criteria drift: a committed reword of criterion 4 is refused', drift(reworded));
    git('checkout', '-q', 'HEAD~1', '--', critRef.path);
    git('commit', '-q', '-am', 'restore');
    check('g2 criteria drift: a committed reorder of criteria 3 and 4 is refused', readG2Criteria(gr).reference.criteriaSha256 === G2_CRITERIA_SHA256 && drift(swap.join('\n')));
  } finally {
    rmSync(gr, { recursive: true, force: true });
  }
  const swapped = [crit[0], crit[1], crit[3], crit[2]];
  check('g2 criteria drift: evaluateG2 itself refuses reordered or reworded criteria', throws(() => evaluateG2({ manifest: { outcome: 'NOT RUN' }, baselineText: BASELINE, criteria: swapped }), CriteriaDriftError) && throws(() => evaluateG2({ manifest: { outcome: 'NOT RUN' }, baselineText: BASELINE, criteria: [...crit.slice(0, 3), `${crit[3]} (edited)`] }), CriteriaDriftError));

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
  // #199: Codex 0.159.2's trust dialog as captured live (the scratch path stands in for
  // <SCRATCH>), without and with its optional Note block.
  const TRUST_BODY = [
    '  Trust this folder? Codex can read, edit, and run files here, subject to your permission settings. Folder settings',
    '  can run code automatically, even without a model request. Continue only if you trust these files. Your trust',
    '  …',
    '› 1. Trust and continue',
    '  2. Back to Agent Command Center',
    '',
    '  enter continue · esc back',
  ];
  const trust = ['  C:\\tmp\\l3-codex-project', '', ...TRUST_BODY].join('\n');
  const trustNote = ['  C:\\tmp\\l3-codex-project', '', '  Note: You’re in a subdirectory of a Git project. Trusting will apply to the repository root:', '  C:\\tmp\\l3-codex-project', '', ...TRUST_BODY].join('\n');
  const plan = (t) => driverMayAcceptCodex(classifyCodexScreen(t));
  const refused = (t) => {
    const p = plan(t);
    return !p.ok && p.keys.length === 0 && p.moves.length === 0;
  };
  const tc = classifyCodexScreen(trust);
  check('g2 pane #199: the recorded trust dialog is recognized, "1. Trust and continue" selected with `›`, and the driver accepts it with `enter` alone', tc.dialog === 'workspace-trust' && tc.selected?.number === 1 && plan(trust).ok && JSON.stringify(plan(trust).keys) === '["enter"]' && plan(trust).moves.length === 0 && tc.options.marked === 1 && tc.options[0].mark === '›', JSON.stringify(plan(trust)));
  check('g2 pane #199: the optional Note block ("subdirectory of a Git project") is body text, not an option: accepted the same way', classifyCodexScreen(trustNote).options?.unknown.length === 0 && JSON.stringify(plan(trustNote).keys) === '["enter"]', JSON.stringify(plan(trustNote)));
  check('g2 pane #199: a second `›` marker (double selection) is refused, no key', refused(trust.replace('  2. Back', '› 2. Back')) && /exactly one selected/.test(plan(trust.replace('  2. Back', '› 2. Back')).why));
  check('g2 pane #199: an extra option (numbered, or unnumbered after the options) is refused, no key', refused(trust.replace('  2. Back to Agent Command Center', '  2. Back to Agent Command Center\n  3. Trust once')) && refused(trust.replace('  2. Back to Agent Command Center', '  2. Back to Agent Command Center\n      Trust for this session only')));
  check('g2 pane #199: "Back" preselected (a different preselection) is refused, no key', refused(trust.replace('› 1.', '  1.').replace('  2. Back', '› 2. Back')) && /not the workspace-trust accepting option/.test(plan(trust.replace('› 1.', '  1.').replace('  2. Back', '› 2. Back')).why));
  check('g2 pane #199: reordered options, swapped numbers, or a changed option text are refused, no key', refused(trust.replace('› 1. Trust and continue\n  2. Back to Agent Command Center', '  1. Back to Agent Command Center\n› 2. Trust and continue')) && refused(trust.replace('› 1. Trust', '› 2. Trust').replace('  2. Back', '  1. Back')) && refused(trust.replace('2. Back to Agent Command Center', '2. Quit')) && refused(trust.replace('1. Trust and continue', '1. Open restricted')));
  check('g2 pane #199: a selection shown with another marker than `›` is refused, no key', refused(trust.replace('› 1.', '❯ 1.')) && /marker/.test(plan(trust.replace('› 1.', '❯ 1.')).why));
  // #201 review: the footer is the recorded line exactly, and required.
  check('g2 pane #201: the sandbox footer ("enter continue and create sandbox · esc back") is refused, no key', refused(trust.replace('enter continue · esc back', 'enter continue and create sandbox · esc back')) && /recorded footer not on screen/.test(plan(trust.replace('enter continue · esc back', 'enter continue and create sandbox · esc back')).why));
  check('g2 pane #201: a pane with no footer, or another footer ("esc quit"), is refused, no key', refused(trust.replace('\n\n  enter continue · esc back', '')) && refused(trust.replace('esc back', 'esc quit')));
  // #201 review: the question paragraph is the recorded one (whole, or a prefix ending in "…").
  check('g2 pane #201: an unnumbered line inserted above option 1 ("Always trust") is refused, no key', refused(trust.replace('› 1. Trust', '  Always trust\n› 1. Trust')) && refused(trust.replace('› 1. Trust', '     Always trust\n› 1. Trust')) && /question text off record/.test(plan(trust.replace('› 1. Trust', '  Always trust\n› 1. Trust')).why));
  check('g2 pane #201: the detect phrase inside unrelated quoted text is not a trust dialog, and is refused', classifyCodexScreen('  Would you like to run: echo "Trust this folder?"\n› 1. Trust and continue\n  2. Back to Agent Command Center\n\n  enter continue · esc back').dialog !== 'workspace-trust' && refused('  Would you like to run: echo "Trust this folder?"\n› 1. Trust and continue\n  2. Back to Agent Command Center\n\n  enter continue · esc back'));
  check('g2 pane #201: a truncated paragraph without "…", or a changed paragraph, is refused; the whole paragraph is accepted', refused(trust.replace('\n  …', '')) && refused(trust.replace('Folder settings', 'Folder hooks')) && plan(trust.replace('  …', '  decision will be saved.')).ok);
  check('g2 pane #201: a Note block with other text than recorded is refused', refused(trustNote.replace('apply to the repository root:', 'apply to the parent folder:')));
  check('g2 pane #201: a selection-marked line below the footer (a stacked dialog) counts as a second marker: refused, no key', refused(`${trust}\n› 1. Yes, proceed`) && refused(`${trust}\n${trust}`));
  check('g2 pane #199: the old guessed trust text (not on record) is refused, no key', refused('Do you trust the files in this folder?\n\n› 1. Yes, continue\n  2. No, quit\n\nPress enter to continue'));
  const unk = classifyCodexScreen('Allow command?\n› 1. Yes\n  2. No\nPress enter to confirm');
  check('g2 pane: an approval or any unnamed prompt is "unknown" and never driver-accepted', unk.dialog === 'unknown' && !driverMayAcceptCodex(unk).ok);
  check('g2 pane: in-progress indicator is a parameter; an idle composer is no dialog', classifyCodexScreen('• Working (3s • esc to interrupt)').busy && classifyCodexScreen('› Ask Codex to do anything').dialog === null && !classifyCodexScreen('› Ask Codex').busy);
  // #204: the startup hook review and hooks browser (seen live on 0.159.2) are recognized and
  // never driver-answered, whatever is selected; the reason names the review and /hooks.
  const REVIEW = ['', '  Hooks need review', '  1 hook is new or changed.', '  Hooks can run outside the sandbox after you trust them.', '', '', '› 1. Review hooks', '  2. Trust all and continue', "  3. Continue without trusting (hooks won't run)", '', '  enter confirm · esc skip'].join('\n');
  const rv = classifyCodexScreen(REVIEW);
  check('g2 pane #204: the startup hook review is recognized ("1. Review hooks" selected) and refused, no key, with the operator\'s action in the reason', rv.dialog === 'hooks-review' && rv.selected?.number === 1 && refused(REVIEW) && refused(REVIEW.replace('› 1.', '  1.').replace('  3. Continue', '› 3. Continue')) && /startup hook review/.test(plan(REVIEW).why) && /\/hooks/.test(plan(REVIEW).why), JSON.stringify(plan(REVIEW)));
  const hb = classifyCodexScreen('  Hooks\n  Lifecycle hooks from config and enabled plugins.\n\n  SessionStart   1   0   1   When a session starts\n\n  t trust all · enter review · esc close');
  check('g2 pane #204: the hooks browser is recognized and refused, no key', hb.dialog === 'hooks-browser' && !driverMayAcceptCodex(hb).ok && driverMayAcceptCodex(hb).keys.length === 0);
  // #204: readiness = a new loaded thread on the wire AND the idle composer on the pane.
  const COMPOSER = '  >_ Codex (v0.159.2)\n\n› Ask Codex to do anything\n\n  ? for shortcuts';
  const rd = (text, loaded, preLoaded = ['old']) => codexReadiness({ text, screen: classifyCodexScreen(text), loaded, preLoaded });
  check('g2 ready #204: the startup-draft composer with no new loaded thread is not ready (named as the startup draft)', !rd(COMPOSER, ['old']).ready && /startup draft/.test(rd(COMPOSER, ['old']).why));
  check('g2 ready #204: the composer and a new loaded thread is ready', rd(COMPOSER, ['old', 'new']).ready && rd(COMPOSER, ['old', 'new']).newThreads.join() === 'new');
  check('g2 ready #204: "Waiting for startup", a dialog, the in-progress indicator, an unreadable list, or no composer is not ready', !rd('› hi\n\n  Waiting for startup  · esc cancel', ['new']).ready && /Waiting for startup/.test(rd('› hi\n\n  Waiting for startup  · esc cancel', ['new']).why) && !rd(REVIEW, ['new']).ready && !rd(`${COMPOSER}\n• Working (1s • esc to interrupt)`, ['new']).ready && !rd(COMPOSER, null).ready && !rd('  Hooks', ['new']).ready);
  // #205 review: a poll's answer is read from that poll's own lines, never an older entry; an
  // unreadable pre-launch baseline stops before anything is read or typed.
  const ll = [{ line: 3, data: ['desktop'], error: null }, { line: 9, data: [], error: { code: -1 } }];
  check('g2 ready #205: loadedSince returns only the poll\'s own answer (null when missing or an error, never a stale entry)', loadedSince(ll, 2) === null && loadedSince(ll.slice(0, 1), 2)?.join() === 'desktop' && loadedSince(ll.slice(0, 1), 3) === null && loadedSince([ll[0], { line: 9, data: ['tui'], error: null }], 5)?.join() === 'tui');
  let stopped = null;
  let touched = false;
  waitCodexReady({ read: async () => { touched = true; return { screen: {}, text: '' }; }, handleDialog: async () => {}, listLoaded: async () => { touched = true; return ['desktop']; }, preLoaded: null, timeoutMs: 1000, pollMs: 100, remainingMs: () => 10000, stop: (reason) => { stopped = reason; throw new Error(reason); } }).catch(() => {});
  check('g2 ready #205: an unreadable pre-launch baseline stops the wait at once (no [] fallback that would count an already-loaded thread as new)', /pre-launch `thread\/loaded\/list` could not be read/.test(stopped ?? '') && !touched, stopped);
  check('g2 ready #205: a composer with no new thread at expiry gives the criterion-1 finding; other blockers do not', /may not have attached to the daemon \(G2 criterion 1\)/.test(codexReadyTimeoutFinding(rd(COMPOSER, ['old'])) ?? '') && codexReadyTimeoutFinding(rd(REVIEW, ['old'])) === null && /2 threads new since the launch/.test(multipleNewThreadsFinding(2)));
  const npm = codexLaunchProof([{ pid: 7, argv: ['node', '/usr/lib/node_modules/@scope/codex/bin/codex.js'] }]);
  check('g2 argv: `node .../bin/codex.js` with nothing after it is a plain launch', npm.found && npm.plain && npm.pid === 7 && npm.codexToken === 'codex.js');
  const ov = codexLaunchProof([{ pid: 3, argv: ['bash'] }, { pid: 8, argv: ['/opt/codex/codex', '-c', 'model=x'] }]);
  check('g2 argv: arguments after codex (a config override) are not plain', ov.found && !ov.plain && JSON.stringify(ov.argsAfterCodex) === '["-c","model=x"]' && ov.pid === 8);
  const win = codexLaunchProof([{ pid: 9, argv: null, commandLine: '"C:\\Program Files\\nodejs\\node.exe" "C:\\Users\\x\\AppData\\Roaming\\npm\\node_modules\\@scope\\codex\\bin\\codex.js" --remote ws://127.0.0.1:1' }]);
  check('g2 argv: a Windows command line is split and checked the same way', win.found && !win.plain && win.argsAfterCodex[0] === '--remote' && splitCommandLine('"a b" c').join('|') === 'a b|c');
  // #243: the Microsoft C runtime argv rules ("Parsing C command-line arguments",
  // learn.microsoft.com, ms.date 2021-12-09): every row of that page's examples table, verbatim,
  // behind a program name.
  const W = (s) => splitWindowsCommandLine(s);
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const MS_TABLE = [
    ['"a b c" d e', ['a b c', 'd', 'e']],
    ['"ab\\"c" "\\\\" d', ['ab"c', '\\', 'd']],
    ['a\\\\\\b d"e f"g h', ['a\\\\\\b', 'de fg', 'h']],
    ['a\\\\\\"b c d', ['a\\"b', 'c', 'd']],
    ['a\\\\\\\\"b c" d e', ['a\\\\b c', 'd', 'e']],
    ['a"b"" c d', ['ab" c d']],
  ];
  const tableBad = MS_TABLE.filter(([cl, want]) => !same(W(`ARGS.EXE ${cl}`), ['ARGS.EXE', ...want]) || !same(splitCommandLine(`ARGS.EXE ${cl}`, { platform: 'win32' }), ['ARGS.EXE', ...want]));
  check('g2 argv #243: every row of Microsoft\'s documented examples table splits to the documented argv', tableBad.length === 0, JSON.stringify(tableBad.map(([cl]) => [cl, W(`ARGS.EXE ${cl}`)])));
  check('g2 argv #243: 2n backslashes + quote -> n backslashes and a delimiting quote; 2n+1 -> n and a literal quote; backslashes elsewhere are literal', same(W('p a\\\\"b c"'), ['p', 'a\\b c']) && same(W('p a\\\\\\"b'), ['p', 'a\\"b']) && same(W('p C:\\dir\\x \\\\server\\share'), ['p', 'C:\\dir\\x', '\\\\server\\share']) && same(W('p "C:\\dir with space\\\\"'), ['p', 'C:\\dir with space\\']) && same(W('p a\\'), ['p', 'a\\']));
  check('g2 argv #243: "" inside a quoted string is one literal quote and the string goes on; "" alone is an empty argument; tabs delimit', same(W('p "a""b c" d'), ['p', 'a"b c', 'd']) && same(W('p "" x'), ['p', '', 'x']) && same(W('p\ta\t\tb  '), ['p', 'a', 'b']));
  check('g2 argv #243: argv[0] has no backslash escaping (quotes toggle and are dropped)', same(W('"C:\\Program Files\\node.exe" x'), ['C:\\Program Files\\node.exe', 'x']) && same(W('"C:\\a b\\"x y'), ['C:\\a b\\x', 'y']) && same(W('C:\\bin\\p\\" z'), ['C:\\bin\\p\\ z']));
  // The PR #242 review's real Win32_Process.CommandLine for the default G4 launch: 3 wrong tokens before #243.
  const g4cl = '"C:\\Program Files\\nodejs\\node.exe" "C:\\Users\\x\\AppData\\Roaming\\npm\\node_modules\\@scope\\codex\\bin\\codex.js" -c "mcp_servers.g4http.url=\\"http://127.0.0.1:1/mcp\\""';
  const g4ov = ['-c', 'mcp_servers.g4http.url="http://127.0.0.1:1/mcp"'];
  const g4p = paneArgv([5], new Map([[5, { pid: 5, ppid: 1, argv: null, commandLine: g4cl }]]), { allow: g4ov, expectArgsAfterCodex: g4ov, platform: 'win32' });
  check('g2 argv #243: the default G4 launch\'s Windows command line splits into the validated overrides and matchesExpected', same(W(g4cl).slice(2), g4ov) && g4p.proof.found && g4p.proof.matchesExpected === true && same(g4p.argv[0].argv, ['node.exe', 'codex.js', ...g4ov]), JSON.stringify(g4p));
  check('g2 argv #243: a value with embedded escaped quotes and spaces stays one argument', same(W('p "features.x=a \\"b\\" c"'), ['p', 'features.x=a "b" c']));
  const wrongEsc = paneArgv([5], new Map([[5, { pid: 5, ppid: 1, argv: null, commandLine: g4cl.replace('url=\\"', 'url="') }]]), { allow: g4ov, expectArgsAfterCodex: g4ov, platform: 'win32' });
  check('g2 argv #243 fail-closed: a non-string or a NUL-carrying command line gives null (no proof); a differently escaped launch does not match', W(null) === null && W(42) === null && W('codex\0 -c x') === null && same(W(''), []) && splitCommandLine(null) === null && !codexLaunchProof([{ pid: 1, argv: null, commandLine: 'codex.exe\0 -c x' }], { platform: 'win32' }).found && wrongEsc.proof.found && wrongEsc.proof.matchesExpected === false, JSON.stringify(wrongEsc.proof));
  check('g2 argv #243: the non-Windows (macOS `ps`) branch is pinned on every OS: quoted runs kept together, quotes stripped, no backslash rules', same(splitCommandLine('"a b" \'c d\' e', { platform: 'darwin' }), ['a b', 'c d', 'e']) && same(splitCommandLine('a\\"b c', { platform: 'darwin' }), ['a\\"b', 'c']));
  // #249 (PR #248 review 5): each row is split under the platform it was read on (row.platform,
  // set by lib/proc.mjs's parsers), not the host's: the `platform` option is a fallback only.
  // Here the fallback is deliberately the OTHER platform, so a host-platform split would fail.
  const winTable = parseWin32ProcessJson(JSON.stringify([{ p: 5, pp: 1, c: '2026-10-02T10:00:02.0000000Z', cl: g4cl }]));
  const winRowP = paneArgv([5], winTable, { allow: g4ov, expectArgsAfterCodex: g4ov, platform: 'darwin' });
  const macOv = ['-c', 'features.x=a b'];
  const macTable = parsePsTable("    6     1 Wed Oct  2 10:00:02 2026 /opt/codex -c 'features.x=a b'\n");
  const macRowP = paneArgv([6], macTable, { allow: macOv, expectArgsAfterCodex: macOv, platform: 'win32' });
  // #249 (PR #251 review 1): an UNTAGGED row (a hand-built table) is split by the caller's
  // fallback, and a tagged row by its own platform, whatever the host is. The host is spoofed
  // as linux and as darwin (process.platform, restored after), so this holds on every OS.
  const untagged = new Map([[5, { pid: 5, ppid: 1, argv: null, commandLine: g4cl }]]);
  const spoofed = (host, fn) => {
    const desc = Object.getOwnPropertyDescriptor(process, 'platform');
    Object.defineProperty(process, 'platform', { ...desc, value: host });
    try {
      return fn();
    } finally {
      Object.defineProperty(process, 'platform', desc);
    }
  };
  const hostRuns = ['linux', 'darwin'].map((host) => spoofed(host, () => ({
    host,
    untaggedWin: paneArgv([5], untagged, { allow: g4ov, expectArgsAfterCodex: g4ov, platform: 'win32' }),
    untaggedHost: paneArgv([5], untagged, { allow: g4ov, expectArgsAfterCodex: g4ov }),
    taggedWin: paneArgv([5], winTable, { allow: g4ov, expectArgsAfterCodex: g4ov }),
    record: processArgv(5, untagged, { platform: 'win32' }),
  })));
  check('g2 argv #249: on a non-Windows host (spoofed linux, darwin) a win32-tagged row splits by the Windows rules, an untagged row by the caller\'s fallback (win32 given: Windows rules; none given: the host\'s), and processArgv leaves an untagged row\'s platform null with `source` naming the platform used', hostRuns.every((h) => h.taggedWin.proof.matchesExpected === true && h.untaggedWin.proof.matchesExpected === true && h.untaggedHost.proof.matchesExpected === false && h.record.platform === null && /Win32_Process/.test(h.record.source)), JSON.stringify(hostRuns.map((h) => [h.host, h.taggedWin.proof.matchesExpected, h.untaggedWin.proof.matchesExpected, h.untaggedHost.proof.matchesExpected, h.record])));
  check('g2 argv #249: paneArgv and the launch proof split each row under its stored platform (a Win32_Process row by the C runtime rules, a ps row by the POSIX-ish rules), whatever the fallback says', processArgv(5, winTable).platform === 'win32' && processArgv(6, macTable).platform === 'darwin' && /Win32_Process/.test(processArgv(5, winTable).source) && /ps command/.test(processArgv(6, macTable).source) && winRowP.proof.matchesExpected === true && same(winRowP.argv[0].argv, ['node.exe', 'codex.js', ...g4ov]) && macRowP.proof.matchesExpected === true && same(macRowP.argv[0].argv, ['codex', ...macOv]), JSON.stringify({ winRowP, macRowP }));
  check('g2 argv: no codex process -> not found (never assumed plain)', !codexLaunchProof([{ pid: 1, argv: ['bash', '-l'] }, { pid: 2, argv: null, commandLine: null }]).found);

  // --- #232: minimized pane argv, read from one process-table snapshot ----------------------
  // A random secret of no known shape, planted in pane descendants' argv (Linux argv and a
  // Windows command line). It must not survive into anything paneArgv returns for a record.
  {
  const SECRET = Array.from(randomBytes(20), (b) => String.fromCharCode(97 + (b % 26))).join('');
  const red = createRedactor();
  check('g2 #232: the planted secret is unknown-shaped (redaction alone leaves it in place)', red.redactValue({ v: SECRET }).value.v === SECRET);
  // Synthetic pids no OS process has: finding them proves the read came from the table.
  const P = 2 ** 31 - 10;
  const table = new Map([
    [P, { pid: P, ppid: 1, argv: ['/bin/bash', '-l'], commandLine: null }],
    [P + 1, { pid: P + 1, ppid: P, argv: ['/usr/bin/node', '/home/u/.npm/bin/codex'], commandLine: null }],
    [P + 2, { pid: P + 2, ppid: P + 1, argv: ['/usr/bin/helper', `--token=${SECRET}`, SECRET], commandLine: null }],
    [P + 3, { pid: P + 3, ppid: P + 1, argv: null, commandLine: `"C:\\Program Files\\x\\helper.exe" --auth ${SECRET}` }],
  ]);
  const pr = processArgv(P + 2, table);
  check('g2 #232 perf: processArgv reads the process-table snapshot, never the OS per pid (synthetic pid found; no table or an unknown pid reads nothing)', pr.argv?.[2] === SECRET && /process table/.test(pr.source) && processArgv(P + 2, null).argv === null && /not readable/.test(processArgv(P + 2, null).source) && processArgv(P + 9, table).argv === null && /not in the process table/.test(processArgv(P + 9, table).source), JSON.stringify(pr.source));
  const pa = paneArgv([P, P + 1, P + 2, P + 3, P + 9], table);
  const paText = JSON.stringify(pa);
  check('g2 #232/#244: minimized records keep expected executable basenames and the codex token; an unexpected executable is an arg0 placeholder; every other argument is a length placeholder', JSON.stringify(pa.argv.map((a) => a.argv)) === JSON.stringify([['bash', '<arg len=2>'], ['node', 'codex'], [arg0Placeholder('/usr/bin/helper'), `<arg len=${8 + SECRET.length}>`, `<arg len=${SECRET.length}>`], [arg0Placeholder('C:\\Program Files\\x\\helper.exe'), '<arg len=6>', `<arg len=${SECRET.length}>`], null]) && pa.argv.every((a) => a.minimized && !('commandLine' in a)), paText);
  // #244 (PR #242 review note C): argv[0] is process-settable on Linux (process.title,
  // setproctitle). Its basename is kept only when it names an expected executable.
  const a0 = (argv0) => minimizeArgv([argv0, 'x'])[0];
  const keptA0 = ['/bin/bash', '-bash', '/usr/bin/zsh', 'sh', 'C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe', 'pwsh.exe', 'C:\\Windows\\system32\\cmd.exe', '/usr/local/bin/node', 'C:\\Program Files\\nodejs\\node.exe', '/opt/bin/codex', 'codex.cmd', '/usr/local/bin/claude', 'claude.exe', '/usr/local/bin/herdr'];
  check('g2 #244: an expected executable (shell, node, harness CLI, herdr) keeps its basename as argv[0]', keptA0.every((x) => a0(x) === x.replace(/\\/g, '/').split('/').pop()), JSON.stringify(keptA0.map(a0)));
  const setTitle = [`sshd: op@pts/${SECRET}`, `worker ${SECRET}`, `/usr/bin/${SECRET}`, `${SECRET}.exe`, 'python3', '/usr/bin/env', 'bash-but-not', 'node=1', ''];
  check('g2 #244: a process-set or unexpected argv[0] (setproctitle text, an unknown helper, a lookalike) is `<arg0 len=N>`, its text nowhere', setTitle.every((x) => a0(x) === arg0Placeholder(x)) && !JSON.stringify(setTitle.map(a0)).includes(SECRET) && !EXPECTED_EXECUTABLE.test('bash-but-not'), JSON.stringify(setTitle.map(a0)));
  check('g2 #244: argsAfterCodex (executable: false) is unaffected: its first token is an argument, not an argv[0]', JSON.stringify(minimizeArgv(['bash', 'x'], { executable: false })) === JSON.stringify(['<arg len=4>', '<arg len=1>']));
  const paRed = red.redactValue(pa);
  check('g2 #232: the planted secret is nowhere in the records, which pass the fail-closed scan unchanged', !paText.includes(SECRET) && reportIsClean(paRed.report) && JSON.stringify(paRed.value) === paText, JSON.stringify(paRed.report));
  check('g2 #232: the codex proof is computed on the full argv; plain survives minimization', pa.proof.found && pa.proof.plain && pa.proof.pid === P + 1 && pa.proof.codexToken === 'codex' && JSON.stringify(pa.proof.argsAfterCodex) === '[]');
  const allow = ['-c', 'mcp_servers.x.url="http://127.0.0.1:1/mcp"'];
  const ovp = paneArgv([P], new Map([[P, { pid: P, ppid: 1, argv: ['/opt/codex', ...allow, '--api-key', SECRET], commandLine: null }]]), { allow, expectArgsAfterCodex: allow });
  check('g2 #232: a non-plain launch keeps only the allowlisted arguments verbatim; an extra argument is a placeholder and fails the expected-overrides match', !ovp.proof.plain && JSON.stringify(ovp.proof.argsAfterCodex) === JSON.stringify([...allow, '<arg len=9>', `<arg len=${SECRET.length}>`]) && ovp.proof.matchesExpected === false && !JSON.stringify(ovp).includes(SECRET), JSON.stringify(ovp.proof));
  const exact = paneArgv([P], new Map([[P, { pid: P, ppid: 1, argv: ['/opt/codex', ...allow], commandLine: null }]]), { allow, expectArgsAfterCodex: allow });
  // The match is decided on the full argv: a process whose argument literally reads like a
  // placeholder does not match an expectation spelled that way.
  const ph = argPlaceholder('x'.repeat(9));
  const lookalike = paneArgv([P], new Map([[P, { pid: P, ppid: 1, argv: ['/opt/codex', '-c', 'y'.repeat(9)], commandLine: null }]]), { allow: ['-c'], expectArgsAfterCodex: ['-c', ph] });
  check('g2 #232: the exact validated overrides match and are kept verbatim; the match never compares placeholders', exact.proof.matchesExpected === true && JSON.stringify(exact.proof.argsAfterCodex) === JSON.stringify(allow) && JSON.stringify(lookalike.proof.argsAfterCodex) === JSON.stringify(['-c', ph]) && lookalike.proof.matchesExpected === false);
  const many = new Map(Array.from({ length: 40 }, (_, i) => [P - i, { pid: P - i, ppid: 1, argv: ['sh'], commandLine: null }]));
  check('g2 #232: at most 32 processes are recorded; minimizeArgv passes null through', paneArgv([...many.keys()], many).argv.length === 32 && minimizeArgv(null) === null);
  }

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
  const nr = evaluateG2({ manifest: { outcome: 'NOT RUN', outcomeReason: 'timed out', scenarioData: {} }, transcriptText: null, paneText: null, baselineText: BASELINE, criteria: crit });
  check('g2 report: a NOT RUN leaves every criterion not evaluable', nr.rows.length === 4 && nr.rows.every((r) => r.score === SCORES.NE && /NOT RUN/.test(r.reason)));
  const V = (o = {}, post = true) => ({ versions: { verified: true, cli: '0.157.1', wire: '0.157.1', daemon: { cliVersion: '0.157.1', appServerVersion: '0.157.1', managedCodexVersion: '0.157.1' }, pinsMinimum: '0.154.0', pinsLastTested: '0.157.1', pinsSource: { workingTreeMatchesHead: true }, ...o }, postRun: { matches: post } });
  check('g2 report: versions verified only when CLI, all daemon fields and the wire report one and the same version, before and after; PINS.md is not part of it (#216)', versionsVerified(V()) && !versionsVerified(V({ wire: '0.158.0' })) && !versionsVerified(V({ daemon: { cliVersion: '0.157.1', appServerVersion: '0.158.0', managedCodexVersion: '0.157.1' } })) && !versionsVerified(V({}, false)) && versionsVerified(V({ pinsSource: { workingTreeMatchesHead: false } })) && versionsVerified(V({ pinsLastTested: '0.159.3' })));
  check('g2 report (#216): matching the last tested version is informational only', versionMatchesLastTested(V()) && !versionMatchesLastTested(V({ pinsLastTested: '0.159.3' })));
  const okRun = { outcome: 'PASS', driver: { commit: 'a'.repeat(40), toolsHerdrDirty: false }, captures: [{ file: 'transcript-x-herdr.jsonl', written: true }, { file: 'pane-x-herdr.txt', written: true }], scenarioData: { g2: { ...V(), fixtures: { transcript: 'transcript-x-herdr.jsonl', pane: 'pane-x-herdr.txt' }, captureNames: { transcript: 'transcript-x-herdr.jsonl', pane: 'pane-x-herdr.txt' }, client: { match: true, workingTreeMatchesHead: true } } } };
  check('g2 report: --write accepts only a verified, clean-driver PASS; a dirty tools/herdr/ is refused (unlike G1\'s generator)', writeRefusal(okRun) === null && /toolsHerdrDirty true/.test(writeRefusal({ ...okRun, driver: { commit: 'a'.repeat(40), toolsHerdrDirty: true } })) && /only a PASS run/.test(writeRefusal({ ...okRun, outcome: 'FAIL' })) && writeRefusal({ ...okRun, scenarioData: { g2: { ...okRun.scenarioData.g2, ...V({}, false) } } }) === null && /^VERSION WARNING: .*no fixture is added/.test(fixtureWithheld({ ...okRun, scenarioData: { g2: { ...okRun.scenarioData.g2, ...V({}, false) } } })) && fixtureWithheld(okRun) === null);
  const manifestAtHead = JSON.parse(spawnSync('git', ['show', `HEAD:${MANIFEST_PATH}`], { cwd: REPO, encoding: 'utf8', maxBuffer: 64 << 20 }).stdout);
  const sch = schemaBlockFor({ version: '0.157.1', codexCommit: 'c', manifestJson: manifestAtHead });
  const sch2 = schemaBlockFor({ version: '0.158.0', codexCommit: 'deadbeef', manifestJson: manifestAtHead });
  check('g2 report: schema block for 0.157.1 reuses MANIFEST.json\'s regenerated-schema record and says the capture is NOT yet validated', sch.upstream?.tree_sha256 && sch.local_generation?.cli === 'codex-cli 0.157.1' && /^NOT yet validated/.test(sch.validated_against));
  check('g2 report: for a version with no schema record, the not-regenerated shape with no hash', sch2.upstream === null && sch2.local_generation === null && sch2.sha256 === null && sch2.commit === 'deadbeef' && /not regenerated/.test(sch2.note));
  check('g2 report: credential-shaped fields are found; the human run\'s client frames carry none', credentialShapedFields({ params: { apiKey: 'x', nested: [{ note: 'Bearer abcdef' }] } }).length === 2 && parseG2Transcript(BASELINE).filter((e) => e.direction === 'client->daemon').every((e) => credentialShapedFields(e.payload).length === 0));
  const tpl = renderReport({ manifest: { outcome: 'NOT RUN', scenarioData: { g2: {} } }, evaluation: nr, diffText: null, date: '2026-10-01', fixtures: null, runManifestName: 'x' });
  check('g2 report #252: the record carries a Verification section (herdr and Harness UNVERIFIED for an empty manifest, slots unfilled), no attestation and no equivalence callout', /^## Verification$/m.test(tpl) && /^- \*\*herdr:\*\* UNVERIFIED — /m.test(tpl) && /^- \*\*Harness:\*\* UNVERIFIED — /m.test(tpl) && /^- \*\*Dialogs:\*\* /m.test(tpl) && /^- \*\*Human actions:\*\* none required by a criterion: no criterion of G2 /m.test(tpl) && /^- \*\*Verified by:\*\* <TO FILL/m.test(tpl) && !/Operator attestation|Attested by|^- \[[ x]\] \*\*herdr/m.test(tpl) && !/Equivalence record\*\* for G/.test(tpl) && /Not verdict-bearing/.test(tpl));
}

// --- lifecycle cases (driver end to end against the fakes) --------------------------------

const today = () => new Date().toISOString().slice(0, 10);
const FAST = ['--param', 'settleMs=300', '--param', 'pollMs=200', '--param', 'listPollMs=500', '--param', 'wireTimeoutMs=10000', '--param', 'turnTimeoutMs=30000', '--param', 'startupTimeoutMs=20000', '--param', 'attachTimeoutMs=15000', '--param', 'readyTimeoutMs=15000', '--param', 'busyQueueTimeoutMs=60000'];
const TRACER = join(HERE, 'fs-trace.mjs');

// A fake `codex` on PATH: a copy of test/fake-codex.mjs named `codex`, so a pane process's
// argv reads `node <base>/bin/codex` like an npm-installed Codex. With trace: every Node
// process the driver starts is traced (test/fs-trace.mjs) into <base>/fs-trace.jsonl.
// plantSecret (#232): the fake TUI starts a child carrying a random secret in its argv and
// writes the secret to <base>/planted-secret.txt (plantedSecret below reads it back).
export function fakeCodexEnv(base, { trace = false, plantSecret = false, ...env } = {}) {
  const bin = join(base, 'bin');
  if (plantSecret) env.FAKE_CODEX_PLANT_SECRET_FILE = join(base, 'planted-secret.txt');
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
  const { criteria: crit } = readG2Criteria(REPO);
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
    check('g2 human: client staged from HEAD, sha256 matches the committed blob; ran unmodified in every mode', g2.client.match && g2.client.copySha256 === COMMITTED_CLIENT_SHA256 && g2.client.copy === '<SCRATCH>/g2-client/client.mjs' && /^list(?:,list){2,},turn,busyqueue,turns$/.test(g2.clientRuns.map((x) => x.mode).join()) && g2.clientRuns.every((x) => x.problems.length === 0 && x.exitCode === 0) && g2.divergence.length === 0, JSON.stringify(g2.clientRuns.map((x) => [x.mode, x.exitCode, x.problems])));
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
    check('g2 human: the pane capture holds the dialog read and the post-delivery reads, keyed to herdr commands', pane.some((s) => s.seq === d.readSeq && /Trust this folder\?/.test(s.text)) && pane.some((s) => s.seq === g2.inject.afterReadSeq) && pane.some((s) => s.seq === g2.busyQueue.afterReadSeq));

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
    check('g2 report CLI: draft printed, not verdict-bearing, Verification section (test-double herdr UNVERIFIED), with the per-connection diff', draft.status === 0 && /Not verdict-bearing/.test(draft.stdout) && /## Method-sequence diff/.test(draft.stdout) && /^- \*\*herdr:\*\* UNVERIFIED — .*test double/m.test(draft.stdout) && /== busyqueue:/.test(draft.stdout) && new RegExp(`Criteria source:.*G2-codex-inject\\.md.*${G2_CRITERIA_SHA256}`).test(draft.stdout), draft.stderr);
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
        check('g2 report CLI --write: draft entries carry every required field, the driver block and a schema block', entries.length === 2 && entries.every((e) => REQUIRED.every((k) => k in e) && e.provider === 'codex' && e.version_matches_pin === true && e.driver.herdr_version === 'herdr 0.9.1' && /^[0-9a-f]{40}$/.test(e.driver.driver_commit) && e.driver.run_manifest === `docs/planning/gates/herdr-runs/G2-${date}.run-manifest.json` && e.schema) && (schemaBlockFor({ version: PIN, codexCommit: null, manifestJson: JSON.parse(read(join(REPO, MANIFEST_PATH))) }).upstream ? entries[0].schema.local_generation?.cli === `codex-cli ${PIN}` && /^NOT yet validated/.test(entries[0].schema.validated_against) : entries[0].schema.upstream === null && /not regenerated/.test(entries[0].schema.note)) && entries[1].schema.applicable === false, JSON.stringify(entries.map((e) => e.schema)));
        const again = spawnSync(process.execPath, [REPORT, '--run', r.outDir, '--write', '--root', root], { encoding: 'utf8', timeout: 20000 });
        check('g2 report CLI --write: never overwrites', again.status === 2 && /refusing to overwrite/.test(again.stderr));
      } else {
        check('g2 report CLI --write (tools/herdr/ dirty in this checkout): refused, nothing written', w.status === 2 && /toolsHerdrDirty/.test(w.stderr) && readdirSync(root).length === 0 && !existsSync(join(r.outDir, 'manifest-entries.draft.json')), w.stderr);
      }
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  // #130: in the delivered turn the fake Codex reads a file and calls an MCP tool on its own
  // (synthetic bodies, shaped like the 2026-10-05 run's frames). The captures written must
  // carry neither body, and the report must score exactly as without them.
  const TOOL_BODIES = SYNTH;
  run('g2 #130: a file read and an MCP tool call in the delivered turn are elided from both captures', { args: ['--param', 'accept=driver', ...FAST], fakeCodex: { FAKE_CODEX_DIALOG: 'none', FAKE_CODEX_TOOL_OUTPUT: JSON.stringify(TOOL_BODIES) } }, (r) => {
    const m = r.manifest;
    check('g2 #130: PASS', r.status === 0 && m.outcome === 'PASS', `${m.outcome} ${m.outcomeReason}`);
    const cap = r.capture(names().transcript) ?? '';
    const pane = r.capture(names().pane) ?? '';
    const bodyLines = [...TOOL_BODIES.output.split('\n'), ...TOOL_BODIES.mcpResult.split('\n')].filter(Boolean);
    const leaks = bodyLines.filter((l) => cap.includes(l) || pane.includes(l) || r.manifestText.includes(l));
    check('g2 #130: no tool-output body line in the transcript, the pane or the run manifest', cap.length > 0 && pane.length > 0 && leaks.length === 0, leaks.join(' | '));
    const marker = (s, kind = 'tool-output') => `<ELIDED ${kind} bytes=${Buffer.byteLength(s, 'utf8')} sha256=${sha256(s)}>`;
    check('g2 #130: the transcript carries the read\'s and the MCP result\'s markers, the command and tool kept', cap.includes(JSON.stringify(marker(TOOL_BODIES.output))) && cap.includes(JSON.stringify(marker(TOOL_BODIES.mcpResult))) && cap.includes(JSON.stringify(TOOL_BODIES.command)) && cap.includes(JSON.stringify(TOOL_BODIES.tool)));
    const firstLine = TOOL_BODIES.output.split('\n')[0];
    check('g2 #130: the pane shows the read\'s first output line as a marker, on its own line, glyph kept', pane.split('\n').some((l) => l.includes(`└ ${marker(firstLine, 'tool-output-line')}`)) && pane.includes(`Ran ${TOOL_BODIES.command}`));
    const capT = m.captures.find((c) => c.file === names().transcript);
    const capP = m.captures.find((c) => c.file === names().pane);
    check('g2 #130: the redaction reports list the elisions (line, bytes, sha256), both captures written clean', capT?.written && capP?.written && capT.redaction.elidedToolOutputs.length >= 6 && capP.redaction.elidedToolOutputLines.length >= 1 && reportIsClean(capT.redaction) && reportIsClean(capP.redaction));
    const f = g2Facts(parseG2Transcript(cap));
    const delivered = f.turnsLists.at(-1)?.turns.find((t) => t.userTexts.some((u) => /second daemon client/.test(u)));
    check('g2 #130: the daemon turn record still lists the delivered turn, its message and answer', !!delivered && delivered.status === 'completed' && delivered.agentMessages.length === 1);
    const ev = evalRun(r);
    check('g2 #130: report scores C1, C2, C4 equivalent; C3 pending the operator (as without tool output)', ev.rows.map((x) => x.score).join('|') === [SCORES.EQ, SCORES.EQ, SCORES.NE, SCORES.EQ].join('|'), JSON.stringify(ev.rows.map((x) => [x.score, x.reason])));
    check('g2 #130: an operator score for C3 applies, from the kept pane lines', evalRun(r, parseOperatorScores([{ n: 3, score: 'equivalent', note: 'pane read shows both messages and answers' }])).rows[2].score === SCORES.EQ);
  });

  // #232: this case also plants a random, unknown-shaped secret in the argv of a pane
  // descendant (a child of the fake Codex TUI). It must reach no record file.
  run('g2 driver accept, Codex settles to unknown (secret planted in a descendant\'s argv)', { args: ['--param', 'accept=driver', ...FAST], fakeCodex: { FAKE_CODEX_DIALOG: 'none', FAKE_CODEX_POST_STATE: 'unknown', plantSecret: true } }, (r) => {
    const m = r.manifest;
    const g2 = m.scenarioData.g2;
    const secretFile = join(r.base, 'planted-secret.txt');
    const secret = existsSync(secretFile) ? read(secretFile) : '';
    check('g2 #232 planted: the secret is unknown-shaped (redaction alone leaves it in place)', /^[a-z]{20}$/.test(secret) && createRedactor().redactValue({ v: secret }).value.v === secret);
    const planted = (g2.paneArgv ?? []).flatMap((pa) => pa.argv).filter((a) => a.minimized && a.argv?.length === 4 && a.argv[3] === argPlaceholder(secret));
    check('g2 #232 planted: the descendant carrying it was recorded, its argv minimized to the executable and length placeholders', planted.length >= 1 && planted.every((a) => !a.argv[0].includes('/') && a.argv.slice(1).every((x) => /^<arg len=\d+>$/.test(x))), JSON.stringify(g2.paneArgv?.map((pa) => pa.argv)));
    const leaked = filesUnder(r.outDir).filter((f) => read(f).includes(secret));
    check('g2 #232 planted: the secret is in neither the run manifest nor any other file the run wrote', secret.length === 20 && !r.manifestText.includes(secret) && leaked.length === 0 && filesUnder(r.outDir).length >= 2, leaked.map((f) => relative(r.outDir, f)).join(','));
    check('g2 #232 planted: herdr\'s process-info answer is recorded as pids only', g2.paneArgv.every((pa) => Object.keys(pa.herdrProcessInfo).join() === 'pane_id,shell_pid,foreground_processes' && pa.herdrProcessInfo.foreground_processes.every((p) => Object.keys(p).join() === 'pid')));
    check('g2 unknown: PASS', r.status === 0 && m.outcome === 'PASS', `${m.outcome} ${m.outcomeReason}`);
    check('g2 unknown: herdr\'s `unknown` state was observed and recorded, with a finding', g2.herdrStates.some((s) => s.state === 'unknown') && m.findings.some((f) => /`unknown`/.test(f) && /nothing re-sent/.test(f)), JSON.stringify(g2.herdrStates));
    check('g2 unknown: it triggered no re-submission -- one operator prompt, each delivery once on the wire', r.prompts.length === 1 && g2.injectionsSent.length === 2 && g2Facts(parseG2Transcript(r.capture(names().transcript))).turnStarts.length === 2);
    check('g2 unknown: no Codex dialog shown (trusted project), so the driver sent no dialog key', g2.dialogs.length === 0 && !m.commands.some((x) => x.role === 'dialog-accept'));
  });

  // #199: under the DEFAULT policy (accept=driver) the Codex trust dialog on record (0.159.2)
  // is accepted by the driver: read first, then `enter` alone, straight after a read.
  const driverAccepted = (m, d) => d?.kind === 'workspace-trust' && d.acceptOrigin === 'driver' && JSON.stringify(d.acceptKeys?.map((k) => k.key)) === '["enter"]' && d.inputBetweenReadAndAccept === 0 && m.commands.find((x) => x.seq === d.acceptSeq - 1)?.argv.includes('read') && m.commands.filter((x) => x.role === 'dialog-accept').length === 1 && d.resolvedSeq > d.acceptSeq;
  run('g2 default policy accepts the Codex trust dialog', { args: FAST, fakeCodex: {} }, (r) => {
    const m = r.manifest;
    const g2 = m.scenarioData.g2;
    check('g2 #199 Codex trust: default policy driver; PASS; one dialog-accept (`enter`) straight after a read; nothing re-sent', r.status === 0 && m.outcome === 'PASS' && g2.acceptPolicy === 'driver' && g2.dialogs.length === 1 && driverAccepted(m, g2.dialogs[0]) && g2.dialogs[0].patternVerified?.includes('0.159.2'), `${r.status} ${m.outcomeReason} ${JSON.stringify(g2.dialogs)}`);
    check('g2 #199 Codex trust: the dialog read is kept verbatim in the pane capture', parseSections(r.capture(names().pane)).some((s) => s.seq === g2.dialogs[0]?.readSeq && /Trust this folder\?/.test(s.text) && /› 1\. Trust and continue/.test(s.text)));
  });

  run('g2 driver accepts the Codex trust dialog with its Note block', { args: FAST, fakeCodex: { FAKE_CODEX_DIALOG: 'trust-note' } }, (r) => {
    const m = r.manifest;
    const g2 = m.scenarioData.g2;
    check('g2 #199 Codex trust + Note: PASS; the "subdirectory of a Git project" lines are body text; accepted with `enter`', r.status === 0 && m.outcome === 'PASS' && driverAccepted(m, g2.dialogs[0]), `${r.status} ${m.outcomeReason}`);
  });

  // Off-record shapes: refused, NOT RUN, no key sent to the dialog.
  for (const [variant, why] of [['trust-double-marker', /exactly one selected/], ['trust-extra-option', /not the ones on record/], ['trust-back-preselected', /not the workspace-trust accepting option/]]) {
    run(`g2 driver refuses the Codex ${variant} dialog`, { args: FAST, fakeCodex: { FAKE_CODEX_DIALOG: variant } }, (r) => {
      const m = r.manifest;
      const g2 = m.scenarioData.g2;
      check(`g2 #199 Codex ${variant}: NOT RUN; no dialog-accept command; the dialog is on record as refused`, r.status === 3 && why.test(m.outcomeReason) && !m.commands.some((x) => x.role === 'dialog-accept') && g2.dialogs[0]?.kind === 'workspace-trust' && g2.dialogs[0]?.acceptOrigin === 'none (driver refused)' && g2.injectionsSent.length === 0, `${r.status} ${m.outcomeReason}`);
    });
  }

  // #216: versions float. A Codex version other than PINS.md's last tested one (CLI, daemon or
  // wire), or below the minimum, is a VERSION WARNING finding and the run proceeds.
  run('g2 drifted version (CLI, daemon and wire) warns and the run proceeds', { args: ['--param', 'accept=driver', ...FAST], fakeCodex: { FAKE_CODEX_DIALOG: 'none', FAKE_CODEX_VERSION: '0.999.0' } }, (r) => {
    const m = r.manifest;
    const g2 = m.scenarioData.g2;
    check('g2 #216 drift: PASS (exit 0), never NOT RUN on a version', r.status === 0 && m.outcome === 'PASS', `${r.status} ${m.outcome} ${m.outcomeReason}`);
    check('g2 #216 drift: five VERSION WARNING findings (CLI, three daemon fields, wire), each naming the last tested version', g2.versions.warnings.length === 5 && m.findings.filter((f) => /^VERSION WARNING \(G2\)/.test(f) && f.includes('reports 0.999.0') && f.includes(`last tested ${PIN}`)).length === 5, JSON.stringify(m.findings));
    check('g2 #216 drift: the daemon started, the TUI launched, the injections were delivered, and the captures name the observed version', daemonStarted(r) && r.calls.some((c) => c.argv.includes('agent')) && g2.injectionsSent.length > 0 && g2.versions.verified === true && g2.versions.matchesLastTested === false && JSON.stringify(g2.fixtures) === JSON.stringify(fixtureNames(today(), '0.999.0')), JSON.stringify(g2.captureNames));
    const p = spawnSync(process.execPath, [REPORT, '--run', r.outDir], { encoding: 'utf8', timeout: 20000 });
    check('g2 #216 drift: the draft lists the warnings and states the policy', p.status === 0 && /Finding: VERSION WARNING \(G2\)/.test(p.stdout) && /version warnings: 5/.test(p.stdout) && /never gated, #216/.test(p.stdout), p.stderr + p.stdout.slice(0, 2000));
  });

  run('g2 below-minimum version warns and the run proceeds', { args: ['--param', 'accept=driver', ...FAST], fakeCodex: { FAKE_CODEX_DIALOG: 'none', FAKE_CODEX_VERSION: '0.150.0' } }, (r) => {
    const m = r.manifest;
    check('g2 #216 below minimum: PASS, with VERSION WARNINGs that say "below the minimum"', r.status === 0 && m.outcome === 'PASS' && m.findings.filter((f) => /^VERSION WARNING/.test(f) && /below the minimum 0\.154\.0/.test(f)).length === 5, `${m.outcome} ${m.outcomeReason} ${JSON.stringify(m.findings)}`);
  });

  run('g2 daemon version differs from the CLI: warns, proceeds, captures stay unverified', { args: ['--param', 'accept=driver', ...FAST], fakeCodex: { FAKE_CODEX_DIALOG: 'none', FAKE_CODEX_DAEMON_VERSION: '0.999.0' } }, (r) => {
    const m = r.manifest;
    const g2 = m.scenarioData.g2;
    check('g2 #216 daemon != CLI: the run is not stopped (the daemon field is a VERSION WARNING), the TUI launched', m.outcome !== 'NOT RUN' && m.findings.some((f) => /^VERSION WARNING \(G2\): `codex app-server daemon version` cliVersion reports 0\.999\.0/.test(f)) && r.calls.some((c) => c.argv.includes('agent')), `${m.outcome} ${m.outcomeReason}`);
    check('g2 #216 daemon != CLI: no fixture-named capture (no single version to name)', g2.fixtures === null && g2.versions.verified === false && m.captures.every((c) => c.file.startsWith('unverified-')) && m.findings.some((f) => /captures stay unverified-\*/.test(f)), JSON.stringify(m.captures.map((c) => c.file)));
  });

  run('g2 wire version differs from the CLI: warns, proceeds, captures stay unverified', { args: ['--param', 'accept=driver', ...FAST], fakeCodex: { FAKE_CODEX_DIALOG: 'none', FAKE_CODEX_WIRE_VERSION: '0.999.0' } }, (r) => {
    const m = r.manifest;
    const g2 = m.scenarioData.g2;
    check('g2 #216 wire != CLI: not stopped on the version; the TUI launched; a wire VERSION WARNING', m.outcome !== 'NOT RUN' && r.calls.some((c) => c.argv.includes('agent')) && m.findings.some((f) => /^VERSION WARNING \(G2\): the wire initialize userAgent reports 0\.999\.0/.test(f)), `${m.outcome} ${m.outcomeReason}`);
    check('g2 #216 wire != CLI: captures unverified-* only', g2.fixtures === null && g2.versions.verified === false && m.captures.length > 0 && m.captures.every((c) => c.file.startsWith('unverified-')), JSON.stringify(m.captures.map((c) => c.file)));
    const root = mkdtempSync(join(tmpdir(), 'oac-g2-report-'));
    try {
      const w = spawnSync(process.execPath, [REPORT, '--run', r.outDir, '--write', '--root', root], { encoding: 'utf8', timeout: 20000 });
      const recDate = g2.date;
      const rec = join(root, `docs/planning/gates/herdr-runs/G2-${recDate}.md`);
      if (m.driver.toolsHerdrDirty === false) {
        check('g2 #216 wire != CLI: --write writes the record and run manifest with a VERSION WARNING, but no fixture and no MANIFEST draft (operator decision on #216)', w.status === 0 && existsSync(rec) && existsSync(join(root, `docs/planning/gates/herdr-runs/G2-${recDate}.run-manifest.json`)) && !existsSync(join(root, FIXTURE_DIR)) && !existsSync(join(r.outDir, 'manifest-entries.draft.json')) && /No fixture written: VERSION WARNING/.test(w.stdout) && /Finding: VERSION WARNING: .*no fixture is added/.test(readFileSync(rec, 'utf8')), w.stdout + w.stderr);
      } else {
        check('g2 #216 wire != CLI: --write is refused only for the dirty tools/herdr/, never for the versions', w.status === 2 && /toolsHerdrDirty/.test(w.stderr) && !/same .*version/.test(w.stderr), w.stderr);
      }
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  // #216 review: a malformed PINS.md Codex row never stops a run.
  let pinsClone = null;
  run('g2 malformed PINS.md row warns and the run proceeds', { args: ['--param', 'accept=driver', ...FAST], fakeCodex: { FAKE_CODEX_DIALOG: 'none' }, prepare: () => (pinsClone = cloneWithPins((t) => t.replace(/^(\| Codex CLI \/ app-server \| [^|]*\| )[^|]*/m, '$1**floating** — see "Version policy" '))) }, (r) => {
    try {
      const m = r.manifest;
      const g2 = m.scenarioData.g2;
      check('g2 #216 malformed PINS.md: PASS (exit 0), not FAIL or NOT RUN', r.status === 0 && m.outcome === 'PASS', `${r.status} ${m.outcome} ${m.outcomeReason}`);
      check('g2 #216 malformed PINS.md: a "could not read" VERSION WARNING; every version check warns PINS.md could not be read; captures still named', m.findings.some((f) => /^VERSION WARNING \(G2\): could not read docs\/planning\/PINS\.md "Codex CLI \/ app-server"/.test(f)) && m.findings.filter((f) => /last tested version could not be read/.test(f)).length === 5 && g2.versions.pinsLastTested === null && g2.versions.verified === true && JSON.stringify(g2.fixtures) === JSON.stringify(fixtureNames(today(), PIN)), JSON.stringify(m.findings));
    } finally {
      if (pinsClone) rmSync(pinsClone, { recursive: true, force: true });
    }
  });

  run('g2 launch with a config override', { args: ['--launch', '["codex","-c","model=other"]', ...FAST], fakeCodex: { FAKE_CODEX_DIALOG: 'none' } }, (r) => {
    check('g2 override: FAIL before anything starts', r.status === 1 && /is not plain `codex`/.test(r.manifest.outcomeReason) && !r.calls.some((c) => c.argv.includes('agent')) && !daemonStarted(r), r.manifest.outcomeReason);
  });

  run('g2 TUI does not attach (embedded server)', { args: ['--param', 'accept=driver', ...FAST, '--param', 'attachTimeoutMs=3000', '--param', 'readyTimeoutMs=3000'], fakeCodex: { FAKE_CODEX_DIALOG: 'none', FAKE_CODEX_NO_ATTACH: '1' } }, (r) => {
    const m = r.manifest;
    const g2 = m.scenarioData.g2;
    // #204: an unattached TUI never loads a thread in the daemon, so the ready wait stops first
    // and the operator's message is never typed.
    check('g2 no attach: NOT RUN at the ready wait (no new loaded thread), the reason naming both causes (startup draft or not attached), nothing typed, nothing delivered', r.status === 3 && /was not ready within 3000 ms/.test(m.outcomeReason) && /no thread new since the launch is loaded/.test(m.outcomeReason) && /startup draft/.test(m.outcomeReason) && /not attached to this daemon/.test(m.outcomeReason) && r.prompts.length === 0 && g2.injectionsSent.length === 0 && g2.thread === null, m.outcomeReason);
    check('g2 no attach: recorded as a criterion-1 finding', m.findings.some((f) => /may not have attached/.test(f) && /G2 criterion 1/.test(f)), JSON.stringify(m.findings));
    check('g2 no attach: the transcript holds only list runs, and the thread/list entries were all dropped (no thread identified)', g2.clientRuns.every((x) => x.mode === 'list') && !/PRIVATE/.test(r.capture(names().transcript)));
  });

  run('g2 client divergence', { args: ['--param', 'accept=driver', ...FAST], fakeCodex: { FAKE_CODEX_DIALOG: 'none', FAKE_CODEX_REJECT: 'thread/queue/add' } }, (r) => {
    const m = r.manifest;
    const g2 = m.scenarioData.g2;
    check('g2 divergence: FAIL, recorded as a divergence and a finding, the client not patched', r.status === 1 && /did not work unmodified/.test(m.outcomeReason) && g2.divergence.length === 1 && /thread\/queue\/add/.test(g2.divergence[0]) && m.findings.some((f) => /divergence/.test(f)), m.outcomeReason);
    check('g2 divergence: thread/queue/add sent once only, never retried', g2Facts(parseG2Transcript(r.capture(names().transcript))).queueAdds.length === 1);
    const ev = evalRun(r);
    check('g2 divergence: every criterion not evaluable', ev.rows.every((x) => x.score === SCORES.NE));
  });

  run('g2 Codex version moves mid-run', { args: ['--param', 'accept=driver', ...FAST], fakeCodex: { FAKE_CODEX_DIALOG: 'none', FAKE_CODEX_POST_CLI_VERSION: '0.158.0' } }, (r) => {
    const m = r.manifest;
    const g2 = m.scenarioData.g2;
    check('g2 mid-run move: recorded as a finding; the captures lose their fixture names (unverified-*)', g2.postRun.matches === false && g2.postRun.cli === '0.158.0' && m.findings.some((f) => /changed during the run/.test(f)) && g2.fixtures === null && g2.versions.verified === false && m.captures.length === 2 && m.captures.every((c) => c.file.startsWith('unverified-')), JSON.stringify(m.captures.map((c) => c.file)));
    const w = spawnSync(process.execPath, [REPORT, '--run', r.outDir, '--write', '--root', join(r.base, 'nowrite')], { encoding: 'utf8', timeout: 20000 });
    const recDate = g2.date;
    const rec = join(join(r.base, 'nowrite'), `docs/planning/gates/herdr-runs/G2-${recDate}.md`);
    if (m.driver.toolsHerdrDirty === false) {
      check('g2 mid-run move: --write writes the record and run manifest with a VERSION WARNING, but no fixture and no MANIFEST draft (operator decision on #216)', w.status === 0 && existsSync(rec) && existsSync(join(join(r.base, 'nowrite'), `docs/planning/gates/herdr-runs/G2-${recDate}.run-manifest.json`)) && !existsSync(join(join(r.base, 'nowrite'), FIXTURE_DIR)) && !existsSync(join(r.outDir, 'manifest-entries.draft.json')) && /No fixture written: VERSION WARNING/.test(w.stdout) && /Finding: VERSION WARNING: .*no fixture is added/.test(readFileSync(rec, 'utf8')), w.stdout + w.stderr);
    } else {
      check('g2 mid-run move: --write is refused only for the dirty tools/herdr/, never for the versions', w.status === 2 && /toolsHerdrDirty/.test(w.stderr) && !/same .*version/.test(w.stderr), w.stderr);
    }
  });

  run('g2 human accept timeout', { args: ['--param', 'accept=human', '--param', 'humanAcceptTimeoutMs=1500', ...FAST], fakeCodex: {} }, (r) => {
    const m = r.manifest;
    const d = m.scenarioData.g2.dialogs[0];
    check('g2 human timeout: NOT RUN, nothing sent to the dialog, nothing delivered', r.status === 3 && /not accepted by the operator/.test(m.outcomeReason) && !m.commands.some((x) => x.seq > d.readSeq && ['operator-input', 'dialog-accept'].includes(x.role)) && m.scenarioData.g2.injectionsSent.length === 0, m.outcomeReason);
  });

  // --- #204: Codex 0.159.2's startup draft and startup hook review ---------------------------
  const promptSeq = (m) => m.commands.find((x) => x.role === 'operator-input' && x.argv.includes('prompt'))?.seq ?? null;
  run('g2 #204 startup draft: the message waits for a verified-ready session', { args: ['--param', 'accept=driver', ...FAST], fakeCodex: { FAKE_CODEX_STARTUP_MS: '2500' } }, (r) => {
    const m = r.manifest;
    const g2 = m.scenarioData.g2;
    const ready = g2.codexReady;
    check('g2 #204 draft: PASS; the ready wait first saw the startup-draft composer (no new loaded thread), then a ready session', r.status === 0 && ready && ready.observations.some((o) => /startup draft/.test(o.why ?? '')) && ready.observations.at(-1).why === null && ready.newThreads === 1 && ready.waitedMs >= 1500, `${r.status} ${m.outcomeReason} ${JSON.stringify(ready)}`);
    check('g2 #204 draft: the message was typed once, only after the ready read; nothing was held as "Waiting for startup"', r.prompts.length === 1 && promptSeq(m) > ready.readSeq && !/Waiting for startup/.test(r.capture(names().pane)), String(promptSeq(m)));
  });

  // #282: the session is loaded on the wire (and the composer idle) while herdr still reports
  // Codex working: the startup settle waits it out, re-checks, and only then is the message typed
  // (after its own #253 baseline).
  run('g2 #282 working after the session is ready: settled, then typed', { args: ['--param', 'accept=driver', ...FAST], fakeCodex: { FAKE_CODEX_STARTUP_MS: '2500', FAKE_CODEX_READY_WORKING_MS: '5000' } }, (r) => {
    const m = r.manifest;
    const g2 = m.scenarioData.g2;
    const s = g2.codexStartupSettle;
    check('g2 #282: PASS; herdr reported Codex working once its session was ready, the driver settled it (idle past that, re-checked), then took an idle #253 baseline and typed once', r.status === 0 && s?.outcome === 'settled' && s.observed.state === 'working' && s.settled.stateChangeSeq > s.observed.stateChangeSeq && g2.operatorInput?.baseline?.state === 'idle' && g2.operatorInput.baseline.seq > s.settled.recheckSeq && promptSeq(m) > g2.operatorInput.baseline.seq && r.prompts.length === 1, `${r.status} ${m.outcomeReason} ${JSON.stringify(s)}`);
  });

  run('g2 #204 hook review: NOT RUN naming it; nothing typed, no key sent to it', { args: ['--param', 'accept=driver', ...FAST], fakeCodex: { FAKE_CODEX_STARTUP_MS: '500', FAKE_CODEX_HOOKS_REVIEW: '1' } }, (r) => {
    const m = r.manifest;
    const g2 = m.scenarioData.g2;
    const d = g2.dialogs.find((x) => x.kind === 'hooks-review');
    check('g2 #204 hook review: NOT RUN (exit 3); the reason names the startup hook review and the operator\'s action', r.status === 3 && /dialog \d+ \(hooks-review\): Codex's startup hook review is on screen/.test(m.outcomeReason) && /the driver did not accept it/.test(m.outcomeReason) && /\/hooks/.test(m.outcomeReason), m.outcomeReason);
    check('g2 #204 hook review: read verbatim and refused; no key sent to it; the operator\'s message never typed; nothing delivered', d && d.acceptOrigin === 'none (driver refused)' && !m.commands.some((x) => x.seq > d.readSeq && ['operator-input', 'dialog-accept'].includes(x.role)) && r.prompts.length === 0 && g2.injectionsSent.length === 0, JSON.stringify(d));
  });

  run('g2 #204 hook review, accept=human: the operator answers it; the run goes on', { args: ['--param', 'accept=human', ...FAST], fakeCodex: { FAKE_CODEX_SELF_ACCEPT_MS: '1200', FAKE_CODEX_HOOKS_REVIEW: '1' } }, (r) => {
    const m = r.manifest;
    const g2 = m.scenarioData.g2;
    const d = g2.dialogs.find((x) => x.kind === 'hooks-review');
    check('g2 #204 hook review human: PASS; the review recorded as answered outside the driver, no key sent', r.status === 0 && d?.acceptOrigin === 'human' && !m.commands.some((x) => x.role === 'dialog-accept') && g2.codexReady?.newThreads === 1, `${r.status} ${m.outcomeReason} ${JSON.stringify(g2.dialogs)}`);
  });

  run('g2 #205 an already-loaded thread (another daemon client) is not "new": a hung startup stays NOT RUN', { args: ['--param', 'accept=driver', ...FAST, '--param', 'readyTimeoutMs=3000'], fakeCodex: { FAKE_CODEX_STARTUP_HANG: '1', FAKE_CODEX_PRELOADED: '1' } }, (r) => {
    const m = r.manifest;
    const g2 = m.scenarioData.g2;
    check('g2 #205 preloaded: the baseline holds the other client\'s thread; NOT RUN at the ready wait, nothing typed', r.status === 3 && g2.preLaunch.loaded?.length === 1 && /was not ready within 3000 ms/.test(m.outcomeReason) && /no thread new since the launch/.test(m.outcomeReason) && r.prompts.length === 0, `${r.status} ${m.outcomeReason}`);
  });

  run('g2 #204 startup never completes: NOT RUN naming the startup draft; nothing typed', { args: ['--param', 'accept=driver', ...FAST, '--param', 'readyTimeoutMs=3000'], fakeCodex: { FAKE_CODEX_STARTUP_HANG: '1' } }, (r) => {
    const m = r.manifest;
    check('g2 #204 hang: NOT RUN (exit 3) at the ready wait, naming the startup draft; no prompt typed', r.status === 3 && /was not ready within 3000 ms/.test(m.outcomeReason) && /startup draft/.test(m.outcomeReason) && r.prompts.length === 0 && m.scenarioData.g2.injectionsSent.length === 0, m.outcomeReason);
  });
  return cases;
}
