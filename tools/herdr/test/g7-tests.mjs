// Checks for tools/herdr/scenarios/g7-codex-recheck.mjs (#470) and its quarantined client
// (docs/planning/gates/fixtures/g7-codex-recheck/client.mjs.throwaway-quarantined). Run by
// run.mjs --self-test (unit half, every OS). Three kinds:
//   - source checks: the client's hard limits and the scenario's, read from the committed text;
//   - judgeG7 on observation sets (mutations of a real run's, one claim each);
//   - the REAL client, and the scenario itself, run against test/fake-g7-daemon.mjs through
//     test/fake-g7-codex.mjs standing in for `codex`. Test doubles: they prove the client and the
//     judgments run, and that the limits hold on the wire; they prove nothing about Codex.
//     No real Codex, daemon or model is touched.

import { spawn, spawnSync } from 'node:child_process';
import { chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { delimiter, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { NotRunError } from '../lib/herdr.mjs';
import { parseG2Transcript, stageClientCopy } from '../lib/g2.mjs';
import scenario, { COMMITTED_CLIENT, COMMITTED_CLIENT_SHA256, CASE_SETS, G7_GATE, G7_NON_VERDICT, g7Names, judgeG7, readObservations, wireVersionsOf } from '../scenarios/g7-codex-recheck.mjs';
import { startFakeG7Daemon } from './fake-g7-daemon.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..', '..');
const read = (p) => readFileSync(p, 'utf8').replace(/\r\n/g, '\n');
const CLIENT = read(join(REPO, COMMITTED_CLIENT));
const SCENARIO = read(join(REPO, 'tools', 'herdr', 'scenarios', 'g7-codex-recheck.mjs'));
const code = (s) => s.replace(/\/\/.*$/gm, '');
const FAKE_CODEX = join(HERE, 'fake-g7-codex.mjs');

// The environment with `bin` first on PATH. Windows spells the variable `Path`; every spelling is dropped
// first, so that the child sees exactly one PATH and it is this one.
function envWith(bin, extra = {}) {
  const env = {};
  for (const [k, v] of Object.entries(process.env)) if (!/^path$/i.test(k)) env[k] = v;
  env.PATH = `${bin}${delimiter}${process.env.PATH ?? process.env.Path ?? ''}`;
  return Object.assign(env, extra);
}

// A `codex` on PATH that is the test double.
function shimDir(dir) {
  const bin = join(dir, 'bin');
  mkdirSync(bin, { recursive: true });
  if (process.platform === 'win32') writeFileSync(join(bin, 'codex.cmd'), `@node "${FAKE_CODEX}" %*\r\n`);
  else {
    writeFileSync(join(bin, 'codex'), `#!/bin/sh\nexec node "${FAKE_CODEX}" "$@"\n`);
    chmodSync(join(bin, 'codex'), 0o755);
  }
  return bin;
}

// Run the real client (copied from the working tree) against a fake daemon. -> { exit, stdout, entries }
function runClient(dir, bin, port, set, quietMs = 150, deadlineMs = null, { killAfterMs = null } = {}) {
  mkdirSync(dir, { recursive: true });
  copyFileSync(join(REPO, COMMITTED_CLIENT), join(dir, 'client.mjs'));
  return new Promise((resolveRun) => {
    const p = spawn(process.execPath, [join(dir, 'client.mjs'), 'cases', join(dir, 'cwd'), set, String(quietMs), ...(deadlineMs === null ? [] : [String(deadlineMs)])], { cwd: dir, env: envWith(bin, { G7_FAKE_PORT: String(port) }), stdio: ['ignore', 'pipe', 'ignore'] });
    let stdout = '';
    p.stdout.on('data', (d) => (stdout += d));
    const timer = setTimeout(() => p.kill(), 90000);
    if (killAfterMs !== null) setTimeout(() => p.kill('SIGTERM'), killAfterMs);
    p.on('exit', (exit) => {
      clearTimeout(timer);
      const tp = join(dir, 'transcript.jsonl');
      resolveRun({ exit, stdout, text: existsSync(tp) ? readFileSync(tp, 'utf8') : '', entries: existsSync(tp) ? parseG2Transcript(readFileSync(tp, 'utf8')) : [] });
    });
  });
}

// A stub of the driver's scenario context: enough for the scenario to run.
function stubCtx(root, params, version = '9.9.9') {
  const captures = new Map();
  const records = new Map();
  const findings = [];
  const literals = [];
  let n = 0;
  return {
    params: { cases: 'all', quietMs: '150', casesTimeoutMs: '90000', cliTimeoutMs: '60000', clientTimeoutMs: '60000', ...params },
    herdr: { abortSignal: undefined, inputHalted: null, abortReason: () => 'aborted' },
    harnessVersion: () => `codex-cli ${version}`,
    dir: (name) => {
      const d = join(root, `${name}-${++n}`);
      mkdirSync(d, { recursive: true });
      return d;
    },
    remainingMs: () => 600000,
    record: (k, v) => records.set(k, v),
    capture: (name, text) => captures.set(name, text),
    finding: (f) => findings.push(f),
    redactLiteral: (v, ph) => literals.push([v, ph]),
    noteScratchHolder: () => {},
    out: { captures, records, findings, literals },
  };
}

// The double must be the `codex` a shell finds first on PATH, or nothing below may run: a client that
// found a real Codex would talk to the operator's real daemon. Checked before any client is started.
function shimIsFirst(bin) {
  const r = spawnSync('codex', ['--version'], { shell: true, encoding: 'utf8', timeout: 30000, env: envWith(bin) });
  return r.status === 0 && /^codex-cli 9\.9\.9\s*$/.test(String(r.stdout));
}

async function withShim(bin, port, extraEnv, fn) {
  const saved = { PATH: process.env.PATH, G7_FAKE_PORT: process.env.G7_FAKE_PORT, G7_FAKE_DAEMON: process.env.G7_FAKE_DAEMON, G7_FAKE_VERSION: process.env.G7_FAKE_VERSION };
  process.env.PATH = `${bin}${delimiter}${process.env.PATH}`;
  process.env.G7_FAKE_PORT = String(port);
  Object.assign(process.env, extraEnv);
  try {
    return await fn();
  } finally {
    for (const [k, v] of Object.entries(saved)) {
      if (v === undefined) delete process.env[k];
      else process.env[k] = v;
    }
  }
}

const verdictOf = (j, id) => j.row66.find((r) => r.id === id)?.verdict;

export async function g7Unit(check) {
  // --- the client's hard limits, from its text ----------------------------------------------------
  const cc = code(CLIENT);
  const allowed = /const ALLOWED_METHODS = \[([^\]]*)\]/.exec(cc)?.[1].match(/'[^']+'/g)?.map((x) => x.slice(1, -1)) ?? [];
  const sentLiterals = [...cc.matchAll(/\.request\('([a-z/]+)'/g)].map((m) => m[1]);
  check('g7 client: it sends only the methods on ALLOWED_METHODS (every request literal is on the list), and the list holds no steer, login, list or read method', allowed.length > 5 && sentLiterals.length > 10 && sentLiterals.every((m) => allowed.includes(m)) && !allowed.some((m) => /steer|login|account|thread\/list|thread\/read|fork|inject|settings\/update/.test(m)), `${allowed.length} allowed; ${[...new Set(sentLiterals)].join(', ')}`);
  check('g7 client: turn/steer appears only in comments', !/turn\/steer/.test(cc) && /turn\/steer/.test(CLIENT));
  check('g7 client: the request guard refuses a method off the list and a thread id the process did not start', /if \(!ALLOWED_METHODS\.includes\(method\)\) throw/.test(cc) && /if \(tid !== undefined && !OWN\.has\(tid\)\) throw/.test(cc) && /if \(typeof id === 'string'\) OWN\.add\(id\)/.test(cc));
  check('g7 client: it sends requests and one notification only, never a response (the only sendJson calls)', (cc.match(/sendJson\(/g) ?? []).length === 2 && /sendJson\(\{ jsonrpc: '2\.0', id, method, params \}\)/.test(cc) && /sendJson\(\{ jsonrpc: '2\.0', method, params \}\)/.test(cc));
  check('g7 client: a server->client request is logged, never answered, and stops the running case', /never answered by this client/.test(cc) && /blocked \?\?= \{ method: msg\.method/.test(cc) && /class CaseStop/.test(cc) && /obs\('case-stopped'/.test(cc));
  check('g7 client: it reads no file and no credential store (only appendFileSync is imported from node:fs)', /import \{ appendFileSync \} from 'node:fs'/.test(cc) && !/readFile|createReadStream|readdir|openSync/.test(cc) && !new RegExp(['auth\\.', 'json|\\.sqlite|roll', 'out|CODEX', '_HOME|key', 'ring'].join('')).test(CLIENT));
  check('g7 client: frames about a thread it did not start, and ids in thread/loaded/list that are not its own, are not recorded', /_g7: 'about a thread this client did not start/.test(cc) && /'<NOT-OWN-THREAD>'/.test(cc) && /p\.method === 'thread\/loaded\/list'/.test(cc));
  check('g7 client: it archives only the threads it started, once each', /const todo = \[\.\.\.OWN\]\.filter\(\(id\) => !archived\.has\(id\)\)/.test(cc) && /thread\/archive/.test(cc) && !/thread\/delete/.test(cc));
  check('g7 client: the row 73 override is one member, approvalPolicy, and the header says the client does what OAC must never do', /approvalPolicy: O/.test(cc) && !/\b(?:model|cwd|sandbox|permissions|config): O\b/.test(cc) && /DELIBERATELY DOING WHAT OAC ITSELF MUST NEVER DO/.test(CLIENT) && /MCPB-CDX-006/.test(CLIENT));
  check('g7 client (N1): every request has a timeout and an upgrade is bounded; no path calls process.exit(3) without archiving (all go through finish)', /const REQUEST_MS = 60000/.test(cc) && /const UPGRADE_MS = 30000/.test(cc) && /setTimeout\(\(\) => \{\n\s*conn\.pending\.delete\(id\)/.test(cc) && !/process\.exit\(3\)/.test(cc) && (cc.match(/void finish\(3,/g) ?? []).length === 2 && /process\.on\(sig, \(\) => void finish\(143/.test(cc) && /deadlineHit/.test(cc) && /await archiveOwn\(\);/.test(cc));
  check('g7 client (N3): a quiet window is followed by a check for a late dispatch before any turn/start of its own', (cc.match(/await settleIdle\(main, T\.id, (?:qFrom|heldFrom), '(?:a1|a2|b)'\);\n\s*(?:next = )?(?:const next = )?(?:await )?runTurn/g) ?? []).length === 3 && /const qFrom = done \? main\.events\.indexOf\(done\) \+ 1 : main\.events\.length;/.test(cc) && /const heldFrom = done \? main\.events\.indexOf\(done\) \+ 1 : main\.events\.length;/.test(cc));
  check('g7 client (B1): the row 73 probe has the three states (with subscribers, without, not loaded) and records the subscriber state of each', /w-subscribers/.test(cc) && /l-subscribers/.test(cc) && /u-subscribers/.test(cc) && /w-loaded-before-resume/.test(cc) && /const wovr = await open\('p-wovr'\);\n\s*const wafter = await open\('p-wafter'\);\n\s*const wun = await main\.request\('thread\/unsubscribe'/.test(cc));
  check('g7 client (N1a): finish() sets a stopping flag that every wait and request honours whatever stopOnBlock says, no thread/start is sent once it is set, and nothing reaches [done] or exit 0 after finish() has begun', /let stopping = false/.test(cc) && /stopping = true;\n\s*for \(const c of connections\)/.test(cc) && /if \(stopping && !cleanup\.on\) throw new CaseStop/.test(cc) && /if \(method === 'thread\/start' && stopping\) throw/.test(cc) && /if \(finishing\) return;[^\n]*\n\s*if \(deadlineHit\)/.test(cc) && /if \(finishing\) await finishing;[^\n]*\n\s*process\.exit\(0\)/.test(cc) && /while \(pendingStarts > 0 && Date\.now\(\) < until\)/.test(cc) && /for \(let round = 0; round < 3; round\+\+\)/.test(cc));
  check('g7 client (N1c): a thread/start that timed out is remembered, and a late answer adds its thread to OWN (so it is archived) and is logged', /conn\.lateStarts\.add\(id\)/.test(cc) && /conn\.lateStarts\.has\(msg\.id\)/.test(cc) && /OWN\.add\(lateId\)/.test(cc) && /obs\('late-thread-start'/.test(cc));
  check('g7 client (N4): the effort fallback fires only on an error that names effort or is invalid-params (-32602), never on a timeout', /\/effort\/i\.test\(String\(e\.message \?\? ''\)\) \|\| e\.code === -32602/.test(cc) && /e && !e\.timedOut && /.test(cc));
  check('g7 client: the override differs from the thread\'s own value', /const overrideFor = \(current\) => \(current === 'untrusted' \? 'on-request' : 'untrusted'\)/.test(cc));

  // --- the scenario's limits ---------------------------------------------------------------------------
  const sc = code(SCENARIO);
  check('g7 scenario: of the daemon verbs it runs only `daemon version`; it never starts, stops or restarts the daemon and never runs `codex mcp`', !/'daemon',\s*'(?:start|stop|restart)'/.test(sc) && (sc.match(/codexCli\(\[/g) ?? []).length === 2 && (sc.match(/codexCli\(\['app-server', 'daemon', 'version'\]/g) ?? []).length === 2 && !/'mcp'|'remove'|'login'/.test(sc));
  check('g7 scenario: it reads no credential, database or stored-history file, sends no keystroke and opens no pane', !new RegExp(['auth\\.', 'json|\\.sqlite|roll', 'out|CODEX', '_HOME|key', 'ring|key', 'chain'].join('')).test(SCENARIO) && !/agentPrompt|sendKeys|paneRun|agentRead|workspaceCreate|startAgent|launch\(/.test(sc));
  check('g7 scenario: a daemon that is not running ends the run NOT RUN, before any client case', /daemonV\.status !== 'running'\) \{\n\s*stop\(/.test(sc) && sc.indexOf("daemonV.status !== 'running'") < sc.indexOf("runClient('cases'"));
  check('g7 scenario: it states the header facts: no TUI, row 66 (b) attempted with its schema source, the override chosen with its source, a deliberate MCPB-CDX-006 breach', /There is no TUI and no operator message/.test(SCENARIO) && /ATTEMPTED because the schema/.test(SCENARIO) && /Why `approvalPolicy` for row 73/.test(SCENARIO) && /ThreadResumeResponse reports/.test(SCENARIO) && /DELIBERATELY DOES WHAT OAC ITSELF MUST NEVER DO \(\[MCPB-CDX-006\]\)/.test(SCENARIO));
  check('g7 labels: its own gate label on every version finding and its own non-verdict text', G7_GATE === 'G7 recheck' && /#470/.test(G7_NON_VERDICT) && /nonVerdictBearing: G7_NON_VERDICT/.test(sc) && (sc.match(/codexVersionWarning\(\{/g) ?? []).length === (sc.match(/gate: G7_GATE \}\)\)/g) ?? []).length && /pinsReadWarning\(pin, G7_GATE\)/.test(sc) && JSON.stringify(CASE_SETS) === '["all","row66","row73"]');
  check('g7 sanitizer: the shared function is the one this scenario imports and applies before the capture is written', /sanitizeTranscript/.test(sc) && sc.indexOf('sanitizeTranscript(readFileSync') < sc.indexOf('ctx.capture(g7.captureNames.transcript'));

  // --- names and the committed client -----------------------------------------------------------------------
  check('g7 names: a verified version gives the fixture name; otherwise unverified-*; a case set sits before -herdr', g7Names('2026-10-31', '0.165.0').transcript === 'transcript-g7recheck-2026-10-31-0.165.0-herdr.jsonl' && g7Names('2026-10-31', null).transcript === 'unverified-transcript-g7recheck-2026-10-31-herdr.jsonl' && g7Names('2026-10-31', '0.165.0', 'row73').transcript === 'transcript-g7recheck-2026-10-31-0.165.0-row73-herdr.jsonl');
  check('g7 versions: wireVersionsOf keeps one entry per distinct version', JSON.stringify(wireVersionsOf([{ userAgentVersion: '1.0.0' }, { userAgentVersion: '1.0.0' }, { userAgentVersion: null }])) === '["1.0.0",null]');
  const tmpStage = mkdtempSync(join(tmpdir(), 'oac-g7-unit-'));
  try {
    const st = stageClientCopy(REPO, COMMITTED_CLIENT, tmpStage);
    check('g7: the client is staged from the blob at HEAD, sha256 = the one the scenario was written for', st.match && st.committedSha256 === COMMITTED_CLIENT_SHA256 && st.workingTreeMatchesHead === true, `${st.committedSha256} vs ${COMMITTED_CLIENT_SHA256}`);
  } catch (e) {
    check('g7: the client is staged from the blob at HEAD, sha256 = the one the scenario was written for', false, e.message);
  } finally {
    rmSync(tmpStage, { recursive: true, force: true });
  }

  // --- the real client against the test double ------------------------------------------------------------------
  const root = mkdtempSync(join(tmpdir(), 'oac-g7-run-'));
  try {
    const bin = shimDir(root);
    const guarded = shimIsFirst(bin);
    check('g7 safety: the test double is the `codex` found first on PATH; if it is not, no client is started (a client must never reach a real daemon from the self-test)', guarded);
    if (!guarded) return;

    // 1. the scenario itself, start to end: every claim confirmed, the override applied
    const d1 = await startFakeG7Daemon();
    let ctx1;
    try {
      ctx1 = stubCtx(root, {});
      await withShim(bin, d1.port, {}, () => scenario.run(ctx1));
    } catch (e) {
      check('g7 scenario against the double: the run completes', false, e.stack);
    }
    if (ctx1?.out.records.has('g7')) {
      const g7 = ctx1.out.records.get('g7');
      const names = [...ctx1.out.captures.keys()];
      const text = names.length ? ctx1.out.captures.get(names[0]) : '';
      check('g7 scenario against the double: the run completes with versions verified (CLI, daemon and wire one version), the fixture-shaped name, and no stop', g7.stoppedAt === null && g7.versions.verified === true && g7.postRun.matches === true && names.length === 1 && /^transcript-g7recheck-\d{4}-\d{2}-\d{2}-9\.9\.9-herdr\.jsonl$/.test(names[0]) && /^codex-tui\/9\.9\.9/.test(g7.versions.wireUserAgent), JSON.stringify({ stop: g7.stoppedAt, names, v: g7.versions.verified }));
      check('g7 scenario against the double: the daemon was only asked for its version (a read-only check), twice, and the client ran hello then cases', g7.daemon.versionBefore.argv.join(' ') === 'codex app-server daemon version' && g7.daemon.versionAfter.argv.join(' ') === 'codex app-server daemon version' && g7.clientRuns.map((r) => r.mode).join() === 'hello,cases');
      const j = g7.judgments;
      check('g7 judgments (all behaving as recorded): row 66 (a) both halves, (b), (c) and (d) CONFIRMED', ['66a-queued-at-interrupt', '66a-add-after-interrupt', '66b-queue-management', '66c-unloaded-thread', '66d-extra-member'].every((id) => verdictOf(j, id) === 'CONFIRMED'), JSON.stringify(j.row66.map((r) => [r.id, r.verdict, r.basis])));
      const br = j.row73.branches;
      check('g7 judgments (the double follows the subscriber rule of rust-v0.166.0): row 73 is judged per branch: with subscribers NOT-APPLIED, with none APPLIED-AND-RETAINED, not loaded APPLIED-AND-RETAINED; no caveat about the control; the verdict is not one answer', br.loadedWithSubscribers.outcome === 'NOT-APPLIED' && br.loadedWithoutSubscribers.outcome === 'APPLIED-AND-RETAINED' && br.unloadedControl.outcome === 'APPLIED-AND-RETAINED' && j.row73.caveats.length === 1 && br.loadedWithSubscribers.facts.subscribers.count === 2 && br.loadedWithoutSubscribers.facts.subscribers.count === 0 && !/loaded thread.*applies/.test(j.row73.basis) && /with subscribers: NOT-APPLIED/.test(j.row73.basis), JSON.stringify(j.row73));
      check('g7 capture: the observations are in the capture; no frame about a thread the client did not start, and no id the double reported for one, reached it', /"direction":"observation"/.test(text) && !/SECRET-FOREIGN-PREVIEW|\/secret\/foreign\/cwd/.test(text) && !/"foreign-thread-1"/.test(text) && /<NOT-OWN-THREAD>/.test(text));
      check('g7 scenario against the double: the final findings hold no contradiction', !ctx1.out.findings.some((f) => /CONTRADICTED/.test(f)), JSON.stringify(ctx1.out.findings));
      const methods = d1.received.map((r) => r.msg.method);
      check('g7 wire (double): every request the client sent is on its list, none is a response, and every thread id named is a thread a client of this double started', d1.received.every(({ msg }) => msg.method !== undefined && (msg.method === 'initialized' || allowed.includes(msg.method))) && !methods.includes('turn/steer') && d1.received.every(({ msg }) => msg.params?.threadId === undefined || d1.threads.has(msg.params.threadId)), `${[...new Set(methods)].join(', ')}`);
      const sentSettings = d1.received.filter(({ msg }) => msg.method === 'thread/resume' && msg.params.approvalPolicy !== undefined);
      const turnStarts = d1.received.filter(({ msg }) => msg.method === 'turn/start');
      check('g7 wire (double): exactly three resumes carry an override (loaded with subscribers, loaded without, not loaded), each only approvalPolicy beyond threadId and excludeTurns; all other resumes carry no setting', sentSettings.length === 3 && sentSettings.every(({ msg }) => Object.keys(msg.params).sort().join() === 'approvalPolicy,excludeTurns,threadId') && d1.received.filter(({ msg }) => msg.method === 'thread/resume').every(({ msg }) => !('model' in msg.params) && !('cwd' in msg.params) && !('sandbox' in msg.params) && !('permissions' in msg.params) && !('config' in msg.params)));
      check('g7 wire (double): every turn/start asks for a low reasoning effort (N4), and none is sent to a thread with a turn running', turnStarts.length >= 10 && turnStarts.every(({ msg }) => msg.params.effort === 'low') && !d1.joinedTurns);
      check('g7 wire (double): the loaded-without-subscribers override was sent while the thread had no subscriber and was still loaded (the double saw it)', d1.overrideLog.some((o) => o.loaded === true && o.hadSubscribers === false && o.idle === true) && d1.overrideLog.some((o) => o.loaded === true && o.hadSubscribers === true) && d1.overrideLog.some((o) => o.loaded === false));
      {
        const entries = parseG2Transcript(text);
        const answers = new Map();
        for (const e of entries) if (e.direction === 'daemon->client' && e.payload?.result?.thread?.id && !answers.has(e.payload.result.thread.id)) answers.set(e.payload.result.thread.id, e.line);
        const startedFrames = entries.filter((e) => e.direction === 'daemon->client' && e.payload?.method === 'thread/started' && e.payload.params?.thread?.id);
        check('g7 held frames (N7): a thread/started that arrives BEFORE the thread/start answer is held and recorded after it, in full, for every thread the client started', answers.size === 6 && [...answers].every(([id, line]) => startedFrames.some((f) => f.payload.params.thread.id === id && f.line > line)));
        const closedModes = new Set(entries.filter((e) => e.direction === 'daemon->client' && e.payload?.method === 'thread/closed').map((e) => e.mode));
        check('g7 broadcast (N7): thread/closed reaches connections other than the one that unsubscribed', closedModes.size > 1 && [...closedModes].some((m) => !['p-main', 'dac-main'].includes(m)), [...closedModes].join());
      }
      check('g7 wire (double): the queue was managed by a connection that never started or resumed the thread', (() => {
        const mgmt = d1.received.filter(({ msg }) => /^thread\/queue\/(list|update|delete|reorder)$/.test(msg.method));
        const ids = new Set(mgmt.map((m) => m.connId));
        const threadId = mgmt[0]?.msg.params.threadId;
        return mgmt.length === 5 && ids.size === 1 && new Set(mgmt.map((m) => m.msg.params.threadId)).size === 1 && !d1.received.some((r) => ids.has(r.connId) && (r.msg.method === 'thread/start' || (r.msg.method === 'thread/resume' && r.msg.params.threadId === threadId)));
      })());
      check('g7 cleanup (double): every thread the client started (six) was archived, and none other', d1.threads.size === 6 && [...d1.threads.values()].every((t) => t.archived));
    }
    await d1.close();

// 2. the double reports the override and keeps nothing; and one that always applies it, and refuses the effort member
    const d2 = await startFakeG7Daemon({ overrideLoaded: 'echo' });
    const r2 = await runClient(join(root, 'echo'), bin, d2.port, 'row73');
    const j2 = judgeG7(readObservations(r2.entries).map, {});
    check('g7 judgments (override only reported): row 73 REPORTED-NOT-RETAINED on both loaded branches, APPLIED-AND-RETAINED when not loaded', r2.exit === 0 && j2.row73.branches.loadedWithSubscribers.outcome === 'REPORTED-NOT-RETAINED' && j2.row73.branches.loadedWithoutSubscribers.outcome === 'REPORTED-NOT-RETAINED' && j2.row73.branches.unloadedControl.outcome === 'APPLIED-AND-RETAINED' && j2.row73.branches.loadedWithSubscribers.facts.notification.length === 0, JSON.stringify(j2.row73));
    await d2.close();
    const d2b = await startFakeG7Daemon({ overrideLoaded: 'applied', rejectEffort: true });
    const r2b = await runClient(join(root, 'applied'), bin, d2b.port, 'row73');
    const o2b = readObservations(r2b.entries);
    const j2b = judgeG7(o2b.map, {});
    check('g7 judgments (override always applied): row 73 APPLIED-AND-RETAINED on every branch, notified on the loaded branch with subscribers', Object.values(j2b.row73.branches).every((b) => b.outcome === 'APPLIED-AND-RETAINED') && j2b.row73.branches.loadedWithSubscribers.facts.notification.length === 3, JSON.stringify(j2b.row73));
    const ts2b = d2b.received.filter(({ msg }) => msg.method === 'turn/start');
    check('g7 effort (N4): a refused effort member is recorded once and the turn is sent again without it, and the client keeps going without it', o2b.map.get('effort-rejected')?.effort === 'low' && ts2b[0].msg.params.effort === 'low' && ts2b[1].msg.params.effort === undefined && ts2b.slice(1).every(({ msg }) => msg.params.effort === undefined) && r2b.exit === 0);
    await d2b.close();

    // 2c. the client's own deadline (N1): the running case stops, the threads are archived, the client exits 0 by itself
    const d2c = await startFakeG7Daemon();
    const r2c = await runClient(join(root, 'deadline'), bin, d2c.port, 'all', 20000, 3000);
    const o2c = readObservations(r2c.entries);
    check('g7 deadline (N1): at its own deadline the client stops the case that was running, skips the rest, archives every thread it started and exits 0 (no hard kill)', r2c.exit === 0 && !!o2c.map.get('deadline-reached') && /deadline/.test(o2c.map.get('case-stopped')?.reason ?? '') && d2c.threads.size >= 1 && [...d2c.threads.values()].every((t) => t.archived) && r2c.stdout.includes('[done]'), JSON.stringify([...o2c.map.keys()].filter((k) => /case-|deadline|archive/.test(k))));
    await d2c.close();
    if (process.platform !== 'win32') {
      const d2d = await startFakeG7Daemon();
      const r2d = await runClient(join(root, 'sigterm'), bin, d2d.port, 'all', 20000, null, { killAfterMs: 2500 });
      check('g7 exit paths (N1): SIGTERM mid-case archives the client\'s threads before it exits (143)', r2d.exit === 143 && d2d.threads.size >= 1 && [...d2d.threads.values()].every((t) => t.archived));
      await d2d.close();
    }
    {
      const dead = await startFakeG7Daemon();
      const port = dead.port;
      await dead.close();
      const r2e = await runClient(join(root, 'deadproxy'), bin, port, 'all', 150);
      check('g7 exit paths (N1): a proxy that cannot reach the daemon ends the client with exit 3 at once (no hang, no threads to leave)', r2e.exit === 3 && r2e.entries.every((e) => e.direction !== 'client->daemon' || e.payload?.method === 'initialize' || e.payload?.method === undefined));
    }

    // 3. approval request, ignored override, no queue management
    const d3 = await startFakeG7Daemon({ approvalAtThread: 1, overrideLoaded: 'ignored', queueMgmt: false });
    const r3 = await runClient(join(root, 'variant'), bin, d3.port, 'all');
    const o3 = readObservations(r3.entries);
    const j3 = judgeG7(o3.map, {});
    check('g7 limit (approval): an approval request stops the case that was running, is recorded, and is never answered (no response frame in what the double received); later cases run', r3.exit === 0 && o3.map.get('case-stopped')?.case === 'a1' && /requestApproval/.test(r3.stdout) && d3.received.every(({ msg }) => msg.method !== undefined) && verdictOf(j3, '66a-queued-at-interrupt') === 'INCONCLUSIVE' && verdictOf(j3, '66c-unloaded-thread') === 'CONFIRMED');
    check('g7 judgments (queue methods missing): row 66 (b) CONTRADICTED with the refused operation named; the dispatch is still observed', verdictOf(j3, '66b-queue-management') === 'CONTRADICTED' && /update/.test(j3.row66.find((r) => r.id === '66b-queue-management').basis) && o3.map.get('b-dispatched').count === 3);
    check('g7 judgments (override ignored): row 73 NOT-APPLIED on both loaded branches; the not-loaded control still APPLIED-AND-RETAINED', j3.row73.branches.loadedWithSubscribers.outcome === 'NOT-APPLIED' && j3.row73.branches.loadedWithoutSubscribers.outcome === 'NOT-APPLIED' && j3.row73.branches.unloadedControl.outcome === 'APPLIED-AND-RETAINED', JSON.stringify(j3.row73));
    await d3.close();

    // 4. judgments from mutated observation sets
    const base = readObservations(parseG2Transcript(ctx1?.out.captures.size ? [...ctx1.out.captures.values()][0] : ''));
    const mut = (patch, { add = {} } = {}) => {
      const m = new Map([...base.map].map(([k, v]) => [k, JSON.parse(JSON.stringify(v))]));
      for (const [k, v] of Object.entries(add)) m.set(k, v);
      for (const [k, f] of Object.entries(patch)) f === null ? m.delete(k) : f(m.get(k), m);
      return judgeG7(m, {});
    };
    check('g7 judge: the unmutated capture\'s observations judge as the run did', base.list.length > 40 && verdictOf(mut({}), '66a-queued-at-interrupt') === 'CONFIRMED');
    check('g7 judge (a1): a turn that starts within the quiet window is CONTRADICTED; a missing observation is INCONCLUSIVE', verdictOf(mut({ 'a1-dispatch-within-quiet-window': (o) => (o.dispatched = true) }), '66a-queued-at-interrupt') === 'CONTRADICTED' && verdictOf(mut({ 'a1-second-queued-turn': null }), '66a-queued-at-interrupt') === 'INCONCLUSIVE');
    check('g7 judge (a1): items that do not run after the next turn, or run out of order, are CONTRADICTED; a turn that was not interrupted is INCONCLUSIVE', verdictOf(mut({ 'a1-first-queued-turn': (o) => (o.ran = false) }), '66a-queued-at-interrupt') === 'CONTRADICTED' && verdictOf(mut({ 'a1-queued-turns': (o) => (o.inOrder = false) }), '66a-queued-at-interrupt') === 'CONTRADICTED' && verdictOf(mut({ 'a1-interrupt-completed': (o) => (o.status = 'completed') }), '66a-queued-at-interrupt') === 'INCONCLUSIVE');
    check('g7 judge (a2): an add that dispatches at once is CONTRADICTED', verdictOf(mut({ 'a2-dispatch-within-quiet-window': (o) => (o.dispatched = true) }), '66a-add-after-interrupt') === 'CONTRADICTED' && verdictOf(mut({ 'a2-queued-turn': (o) => (o.afterNextTurn = false) }), '66a-add-after-interrupt') === 'CONTRADICTED');
    check('g7 judge (c): a thread that did not unload is INCONCLUSIVE; an add that loads it, or a resume that does not dispatch it, is CONTRADICTED', verdictOf(mut({ 'c-thread-closed': (o) => (o.closed = false) }), '66c-unloaded-thread') === 'INCONCLUSIVE' && verdictOf(mut({ 'c-loaded-after-add': (o) => (o.loaded = true) }), '66c-unloaded-thread') === 'CONTRADICTED' && verdictOf(mut({ 'c-queued-turn': (o) => (o.ran = false) }), '66c-unloaded-thread') === 'CONTRADICTED' && verdictOf(mut({ 'c-add': (o) => ((o.accepted = false), (o.error = { code: -32600 })) }), '66c-unloaded-thread') === 'CONTRADICTED');
    check('g7 judge (d): a refused extra member, or a changed text, is CONTRADICTED', verdictOf(mut({ 'd-add': (o) => ((o.accepted = false), (o.error = { code: -32602 })) }), '66d-extra-member') === 'CONTRADICTED' && verdictOf(mut({ 'd-turn': (o) => (o.textMatches = false) }), '66d-extra-member') === 'CONTRADICTED');
    check('g7 judge (b): a refused delete, a queue not held, a reorder missing from dispatch, or a deleted item that ran, are not CONFIRMED, each for its stated reason', (() => {
      const del = mut({ 'b-delete': (o) => ((o.error = { code: -32600, message: 'x' }), (o.deleted = false)) }).row66.find((r) => r.id === '66b-queue-management');
      const held = mut({ 'b-held-during-management': (o) => (o.turnStartedMeanwhile = true) }).row66.find((r) => r.id === '66b-queue-management');
      const reo = mut({ 'b-dispatched': (o) => (o.texts = [...o.texts].reverse()) }).row66.find((r) => r.id === '66b-queue-management');
      const ran = mut({ 'b-dispatched': (o) => (o.deletedItemRan = true) }).row66.find((r) => r.id === '66b-queue-management');
      return del.verdict === 'CONTRADICTED' && /delete/.test(del.basis) && held.verdict === 'INCONCLUSIVE' && reo.verdict === 'CONTRADICTED' && ran.verdict !== 'CONFIRMED';
    })());
check('g7 judge (row 73): per branch: an error is REFUSED; a response that does not show the override is NOT-APPLIED; a state that was not reached is INCONCLUSIVE; a failed control adds a caveat; a branch never decides another', (() => {
      const w = (patch) => mut(patch).row73.branches;
      return w({ 'l-resume-override': (o) => (o.error = { code: -32602 }) }).loadedWithSubscribers.outcome === 'REFUSED' &&
        w({ 'w-resume-override': (o) => (o.settings.approvalPolicy = 'on-request') }).loadedWithoutSubscribers.outcome === 'NOT-APPLIED' &&
        w({ 'w-resume-override': (o) => (o.settings.approvalPolicy = 'on-request') }).unloadedControl.outcome === 'APPLIED-AND-RETAINED' &&
        w({ 'l-loaded': (o) => (o.loaded = false) }).loadedWithSubscribers.outcome === 'INCONCLUSIVE' &&
        w({ 'w-loaded-before-resume': (o) => (o.loaded = false) }).loadedWithoutSubscribers.outcome === 'INCONCLUSIVE' &&
        /no longer in thread\/loaded\/list/.test(w({ 'w-loaded-before-resume': (o) => (o.loaded = false) }).loadedWithoutSubscribers.basis) &&
        w({ 'u-thread-closed': (o) => (o.closed = false) }).unloadedControl.outcome === 'INCONCLUSIVE' &&
        w({ 'w-resume-readback': (o) => (o.settings.approvalPolicy = 'on-request') }).loadedWithoutSubscribers.outcome === 'REPORTED-NOT-RETAINED' &&
        mut({ 'u-resume-readback': (o) => (o.settings.approvalPolicy = 'on-request') }).row73.caveats.some((c) => /control/.test(c)) &&
        mut({ 'w-subscribers': null }).row73.branches.loadedWithoutSubscribers.outcome === 'INCONCLUSIVE';
    })());
    check('g7 judge (row 73): no basis or claim states a general answer for a loaded thread; the NOT-APPLIED basis names its state', !/already loaded\b.*(applies|ignores)/.test(mut({}).row73.claim) && /\(loaded, idle, with subscribers\)/.test(mut({}).row73.branches.loadedWithSubscribers.basis) && /subscribers/.test(mut({}).row73.claim));
    check('g7 judge (N3): a dispatch that started after the quiet window is CONTRADICTED for (a) and makes (b) INCONCLUSIVE', verdictOf(mut({ 'a1-late-dispatch': () => {}, }, { add: { 'a1-late-dispatch': { turnIds: ['t'] } } }), '66a-queued-at-interrupt') === 'CONTRADICTED' && verdictOf(mut({}, { add: { 'a2-late-dispatch': { turnIds: ['t'] } } }), '66a-add-after-interrupt') === 'CONTRADICTED' && verdictOf(mut({}, { add: { 'b-late-dispatch': { turnIds: ['t'] } } }), '66b-queue-management') === 'INCONCLUSIVE');
    check('g7 judge (W1): the loaded-without-subscribers state needs unsubscribed status AND an idle thread; notSubscribed, notLoaded, a first turn that did not complete (with no idle thread status in the answer) are INCONCLUSIVE, each for its reason; (b) of it also applies to the subscribed state', (() => {
      const w = (patch) => mut(patch).row73.branches;
      const idleAns = (o) => (o.settings.threadStatus = { type: 'idle' });
      const notIdle = (o) => (o.settings.threadStatus = { type: 'active' });
      return w({ 'w-unsubscribe': (o) => (o.status = 'notSubscribed') }).loadedWithoutSubscribers.outcome === 'INCONCLUSIVE' &&
        /notSubscribed/.test(w({ 'w-unsubscribe': (o) => (o.status = 'notSubscribed') }).loadedWithoutSubscribers.basis) &&
        w({ 'w-unsubscribe': (o) => (o.status = 'notLoaded') }).loadedWithoutSubscribers.outcome === 'INCONCLUSIVE' &&
        w({ 'w-first-turn': (o) => (o.status = 'failed'), 'w-resume-override': notIdle }).loadedWithoutSubscribers.outcome === 'INCONCLUSIVE' &&
        w({ 'w-first-turn': (o) => (o.status = 'failed'), 'w-resume-override': idleAns }).loadedWithoutSubscribers.outcome !== 'INCONCLUSIVE' &&
        w({ 'l-first-turn': (o) => (o.status = 'failed'), 'l-resume-override': notIdle }).loadedWithSubscribers.outcome === 'INCONCLUSIVE' &&
        w({ 'l-first-turn': (o) => (o.status = 'failed'), 'l-resume-override': idleAns }).loadedWithSubscribers.outcome !== 'INCONCLUSIVE' &&
        w({ 'w-first-turn': (o) => (o.status = 'failed'), 'w-resume-override': notIdle }).loadedWithSubscribers.outcome === 'NOT-APPLIED';
    })());
    check('g7 judge (N4b): a next turn that did not complete makes (a) and (b) INCONCLUSIVE with that basis, not CONTRADICTED', (() => {
      const a1 = mut({ 'a1-first-queued-turn': (o) => (o.ran = false) }, { add: { 'a1-next-turn': { status: 'failed', error: null } } }).row66.find((r) => r.id === '66a-queued-at-interrupt');
      const a2 = mut({ 'a2-queued-turn': (o) => (o.ran = false) }, { add: { 'a2-next-turn': { status: 'interrupted', error: null } } }).row66.find((r) => r.id === '66a-add-after-interrupt');
      const b = mut({ 'b-next-turn': (o) => (o.status = 'failed'), 'b-dispatched': (o) => (o.texts = []) }).row66.find((r) => r.id === '66b-queue-management');
      const a1ok = mut({ 'a1-first-queued-turn': (o) => (o.ran = false) }, { add: { 'a1-next-turn': { status: 'completed', error: null } } }).row66.find((r) => r.id === '66a-queued-at-interrupt');
      return a1.verdict === 'INCONCLUSIVE' && /next uninterrupted turn ended .failed./.test(a1.basis) && a2.verdict === 'INCONCLUSIVE' && /interrupted/.test(a2.basis) && b.verdict === 'INCONCLUSIVE' && /uninterrupted turn after the changes ended .failed./.test(b.basis) && a1ok.verdict === 'CONTRADICTED';
    })());

    // 5. no running daemon: NOT RUN, nothing sent
    const d4 = await startFakeG7Daemon();
    const ctx4 = stubCtx(root, {});
    let notRun = null;
    try {
      await withShim(bin, d4.port, { G7_FAKE_DAEMON: 'stopped' }, () => scenario.run(ctx4));
    } catch (e) {
      notRun = e;
    }
    check('g7 scenario: with no running daemon it ends NOT RUN, naming the operator as the one who starts it, and the double saw no client at all', notRun instanceof NotRunError && /not running/.test(notRun.reason ?? notRun.message) && /never starts, stops or restarts/.test(notRun.reason ?? notRun.message) && d4.received.length === 0, String(notRun?.reason ?? notRun?.message ?? notRun));
    await d4.close();
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}
