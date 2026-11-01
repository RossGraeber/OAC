// Checks for tools/herdr/scenarios/g5-claude-resume.mjs (#472), run by run.mjs --self-test (unit half, every OS).
//
// The scenario itself, start to end, against a TEST DOUBLE of the panes: a herdr object whose three panes
// (the example live daemon, `oac sessions --json`, Claude Code in print mode) are simulated in memory, in
// stages that the scenario's own waits release. Nothing here starts a process, reads a credential or reaches
// a harness, a daemon or herdr; it proves what the scenario does with the text it is given, and nothing about
// Claude Code, the OAC daemon or herdr. The ids, event lines and session lists are shaped like the committed
// fixtures in docs/planning/gates/fixtures/g5-claude-resume/ (a run's text, not their bytes).
//
// The double has no timer: a wait that no staged event can satisfy fails at once, as herdr's would at its
// timeout (and, as the driver does after a timeout, input to panes is then refused). So these checks also show
// that the scenario's waits are events, not sleeps.

import { mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, appendFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { quoteCommand } from '../lib/pane-shell.mjs';
import scenario, {
  EXPECTED_CONNECTIONS, MAX_BIND_RETRIES, oacIdsIn, unixMsAfterPattern, eventAfterRegex, freedAfterRegex, daemonClockMs, firstFreedAfter, sliceFromToken, hasLine, claudeResults, sessionIdsIn, summarizeRun, judgeG5, daemonSummary, daemonEvents,
} from '../scenarios/g5-claude-resume.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..', '..');
const SCENARIO_FILE = join(REPO, 'tools', 'herdr', 'scenarios', 'g5-claude-resume.mjs');
const FIXTURES = join(REPO, 'docs', 'planning', 'gates', 'fixtures', 'g5-claude-resume');

const BASE_MS = 1793421493982;
const LABELS = ['A', 'B', 'C'];
const CLAUDE_ID = '3f0c1e52-9a77-4c1d-8d2e-5b6a7c8d9e01';
const claudeIdOf = (L, distinct) => (distinct ? `3f0c1e52-9a77-4c1d-8d2e-5b6a7c8d9e0${LABELS.indexOf(L) + 1}` : CLAUDE_ID);
// OAC session ids have the shape core/src/ids.rs is_session_id checks: 26 characters, the first 0-7, the rest Crockford base32.
const oacIdOf = (L) => `3qvr6ndjfdd6qp3h38ajjakan${LABELS.indexOf(L) + 1}`;
const extraIdOf = (L) => `7${'z'.repeat(23)}${L.toLowerCase()}9`;
const PROSE_ID = `5${'p'.repeat(25)}`;
const LOG_ID = `6${'q'.repeat(25)}`;

class Timeout extends Error {}
class Refused extends Error {}

// The simulated panes. opts (all optional):
//   distinctClaudeIds, sleepFailed:[L], noResult:[L], extraSession:L, state, harness, unboundDuring, unboundAfter,
//   staleAfter:L, dropAccepted:L, findingLine:L, sc142:L, doneMissing:L, markEarly:L, sameOacId, selfStop,
//   daemonNeverReady, readyWaitError
function makeWorld(root, opts = {}) {
  let ms = BASE_MS;
  const panes = new Map();
  const log = { runs: [], waits: [], stopsTyped: 0, stopRefused: 0, claudeCommands: [], opsCommands: 0 };
  let halted = false;
  let daemonRunning = false;
  let eventsFile = null;
  let session = null; // the bound session now listed, or null
  let unbound = 0;
  let queue = [];
  let pendingBind = null;
  let current = null;
  const pane = (id) => panes.get(id);
  const add = (id, line) => pane(id).lines.push(line);
  const daemonPane = () => [...panes.values()].find((p) => p.role === 'daemon');
  const claudePane = () => [...panes.values()].find((p) => p.role === 'claude');

  const emit = (event, body = {}) => {
    ms += 5;
    const line = JSON.stringify({ event, unix_ms: ms, ...body });
    daemonPane().lines.push(line);
    if (eventsFile) appendFileSync(eventsFile, `${line}\n`);
  };
  const emitLog = (line) => emit('log', { line: `oac-daemon: ${line}` });
  const peer = (n) => ({ pid: 4000 + n, started: 1 });

  function sessionsJson() {
    const list = [];
    if (session) {
      list.push({ active_inbound: true, content_types: [], harness: opts.harness ?? 'claude', major: 0, max_envelope_octets: 65536, session_id: session.id, state: opts.state ?? 'online' });
      if (opts.extraSession === session.label) list.push({ harness: 'claude', session_id: extraIdOf(session.label), state: 'online' });
    }
    return JSON.stringify({ oac_cli: 1, ok: true, running: true, sessions: list, sessions_total: list.length, unbound: [{ harness: 'claude', unbound_attachments: unbound }, { harness: 'codex', unbound_attachments: 0 }] });
  }

  function startRun(label, args) {
    const index = LABELS.indexOf(label);
    const cid = claudeIdOf(label, opts.distinctClaudeIds);
    current = { label, index, cid, hookSeen: false };
    log.runs.push({ label, args });
    unbound = 1;
    emit('accepted', { label: 'claude', peer: peer(index * 2 + 1), chain: [{ pid: 1, image: 'claude.exe' }], paired: false });
    queue = [
      // stage 2: the hook handler pairs, ends and is freed; the session is bound
      () => {
        if (opts.dropAccepted !== label) emit('accepted', { label: 'claude', peer: peer(index * 2 + 2), chain: [{ pid: 2, image: 'bash.exe' }], paired: false });
        emit('ended', { label: 'claude', peer: peer(index * 2 + 2) });
        emitLog('freed a connection on claude');
        if (opts.sc142 === label) emitLog('binding Diagnostic SC-ID-142 Bound attachment=Some(ConnectionHandle(#5)) session=None');
        if (opts.logId === label) emitLog(`binding Diagnostic SC-ID-140 Bound attachment=Some(ConnectionHandle(#5)) session=Some(SessionId("${LOG_ID}"))`);
        if (opts.findingLine === label) emitLog('binding Finding SC-ID-130 Withheld attachment=Some(ConnectionHandle(#5)) session=None');
        const bind = () => {
          session = { label, id: opts.sameOacId ? oacIdOf('A') : oacIdOf(label) };
        };
        unbound = opts.unboundDuring ?? 0;
        // The bind decision can come after the hook's freed line (the committed run C's does, by 18 ms).
        if (opts.lateBind || opts.neverBind) pendingBind = { after: opts.neverBind ? Infinity : opts.lateBind, bind };
        else bind();
        if (opts.markEarly === label) finishPane();
      },
      // stage 3: Claude Code finishes (the done marker is printed) ...
      () => {
        if (opts.markEarly !== label) finishPane();
        if (opts.doneMissing === label) {
          queue.length = 0;
          return;
        }
        if (!opts.lateFreed) shimEnds();
      },
      // ... and the shim's connection ends and is freed, after the marker when lateFreed; nothing is listed any more
      () => {
        if (opts.lateFreed) shimEnds();
      },
    ];
    const shimEnds = () => {
      {
        emit('ended', { label: 'claude', peer: peer(index * 2 + 1) });
        emitLog('freed a connection on claude');
        pendingBind = null;
        if (opts.staleAfter !== label) session = null;
        unbound = opts.unboundAfter ?? 0;
        if (opts.selfStop && label === 'C') {
          emit('stopped');
          daemonRunning = false;
        }
      }
    };
  }
  function finishPane() {
    const c = current;
    const p = claudePane();
    if (!c || c.finished) return;
    c.finished = true;
    if (opts.sleepFailed?.includes(c.label)) p.lines.push('sleep: missing operand');
    if (!opts.noResult?.includes(c.label)) p.lines.push(JSON.stringify({ type: 'result', subtype: 'success', is_error: false, session_id: c.cid, result: `whoami ${oacIdOf(c.label)}${opts.proseId === c.label ? ` ${PROSE_ID}` : ''}` }));
    if (opts.doneMissing !== c.label) p.lines.push(c.mark);
    p.lines.push('PS C:\\scratch\\claude-cwd>');
  }

  const herdr = {
    abortSignal: undefined,
    inputHalted: null,
    async workspaceCreate({ label }) {
      const id = `p${panes.size + 1}`;
      const role = /daemon/.test(label) ? 'daemon' : /ops/.test(label) ? 'ops' : 'claude';
      panes.set(id, { id, role, lines: [] });
      return { workspaceId: `w${panes.size}`, tabId: `t${panes.size}`, paneId: id };
    },
    async paneRun(id, command) {
      const p = pane(id);
      if (halted) {
        if (command === 'stop') log.stopRefused += 1;
        throw new Refused('input is halted after a timeout');
      }
      if (p.role === 'daemon') {
        if (command === 'stop') {
          log.stopsTyped += 1;
          if (daemonRunning) {
            emitLog('shut down: withdrawals sent, transport shut down');
            emit('stopped');
            daemonRunning = false;
          }
          return;
        }
        add(id, `PS C:\\scratch\\g5r> ${command}`);
        const em = /--events (?:'([^']*)'|(\S+))/.exec(command);
        eventsFile = em ? (em[1] ?? em[2]) : null;
        if (eventsFile) writeFileSync(eventsFile, '');
        daemonRunning = true;
        if (!opts.daemonNeverReady) {
          emit('local_grant', { writer: 'own device key' });
          emitLog('serving pipe-claude, pipe-codex, pipe-control');
          emit('ready', { namespace: 'g5r', endpoints: [], pid: 1 });
        }
        return;
      }
      if (p.role === 'ops') {
        const m = /^echo (G5R-[^ ;]+)-BEGIN; (.*); echo (G5R-[^ ;]+)-EXIT-(\$\?|\$LASTEXITCODE)$/.exec(command);
        if (!m) throw new Error(`ops pane: unexpected command ${command}`);
        log.opsCommands += 1;
        if (pendingBind) {
          if (pendingBind.after <= 0) {
            pendingBind.bind();
            pendingBind = null;
          } else pendingBind.after -= 1;
        }
        add(id, `PS C:\\scratch\\g5r> ${command}`);
        add(id, `${m[1]}-BEGIN`);
        add(id, 'oac sessions: using the test namespace "g5r"');
        add(id, sessionsJson());
        add(id, `${m[3]}-EXIT-0`);
        return;
      }
      const m = /^echo (G5R-([ABC])-BEGIN-[0-9a-f]+); (.*); echo (G5R-[ABC]-DONE-[0-9a-f]+)$/.exec(command);
      if (!m) throw new Error(`claude pane: unexpected command ${command}`);
      add(id, `PS C:\\scratch\\claude-cwd> ${command}`);
      add(id, m[1]);
      log.claudeCommands.push(m[3]);
      startRun(m[2], m[3]);
      current.mark = m[4];
    },
    async paneRead(id, { lines } = {}) {
      let all = pane(id).lines;
      if (pane(id).role === 'claude' && opts.hideBegin && current?.label === opts.hideBegin) all = all.filter((l) => !l.includes(`G5R-${opts.hideBegin}-BEGIN-`));
      return (lines ? all.slice(-lines) : all).join('\n');
    },
    async paneWaitOutput(id, { regex, match, source }) {
      const p = pane(id);
      const re = regex ? new RegExp(regex) : null;
      log.waits.push({ role: p.role, regex: regex ?? null, match: match ?? null, source: source ?? null });
      if (opts.readyWaitError && /"event":"ready"/.test(regex ?? '')) throw new Error('herdr: the pane is gone');
      for (;;) {
        if (p.lines.some((l) => (re ? re.test(l) : l.includes(match)))) return { type: 'pane_output_matched' };
        const next = queue.shift();
        if (!next) {
          halted = true;
          throw new Timeout(`timed out waiting for ${regex ?? match}`);
        }
        next();
      }
    },
  };
  return { herdr, log, panes, ms: () => ms };
}

// The scenario's ctx: captures are redacted AT THE TIME THEY ARE WRITTEN with the literals registered so far,
// the strictest reading of "early", so a value registered after its capture shows up raw.
function makeCtx(world, root, params = {}, shell = 'posix') {
  const captures = new Map();
  const records = new Map();
  const findings = [];
  const literals = [];
  let n = 0;
  const redact = (text) => literals.reduce((t, [v, ph]) => t.split(v).join(ph), text);
  return {
    herdr: world.herdr,
    params: { ...scenario.defaults.params, oac: 'FAKE-OAC', daemon: 'FAKE-DAEMON', ...params },
    paneShell: shell,
    dir: (name) => {
      const d = join(root, `${name}-${++n}`);
      mkdirSync(d, { recursive: true });
      return d;
    },
    quoteForPane: (argv) => quoteCommand(argv, shell),
    probeEnv: async () => ({}),
    record: (k, v) => records.set(k, v),
    capture: (name, text) => {
      if (captures.has(name)) throw new Error(`capture ${name} written twice`);
      captures.set(name, redact(text));
    },
    finding: (f) => findings.push(f),
    redactLiteral: (v, ph) => literals.push([v, ph]),
    out: { captures, records, findings, literals },
  };
}

export async function runScenario(root, opts = {}, params = {}, shell = 'posix') {
  const world = makeWorld(root, opts);
  const ctx = makeCtx(world, root, params, shell);
  const t = Date.now();
  let error = null;
  try {
    await scenario.run(ctx);
  } catch (e) {
    error = e;
  }
  return { world, ctx, error, ms: Date.now() - t, out: ctx.out };
}

const allIds = (opts = {}) => [
  ...LABELS.map((L) => oacIdOf(L)),
  ...LABELS.map((L) => extraIdOf(L)),
  PROSE_ID,
  LOG_ID,
  ...LABELS.map((L) => claudeIdOf(L, opts.distinctClaudeIds)),
];
const rawLeaks = (r, opts) => {
  const text = [...r.out.captures.values()].join('\n') + JSON.stringify([...r.out.records]) + r.out.findings.join('\n');
  return allIds(opts).filter((id) => text.includes(id));
};

export async function g5resumeUnit(check) {
  const src = readFileSync(SCENARIO_FILE, 'utf8').replace(/\r\n/g, '\n');
  const code = src.replace(/\/\/.*$/gm, '');

  // --- pure helpers --------------------------------------------------------------------------------------
  {
    const rnd = (digits) => String(Math.floor(Math.random() * 9 * 10 ** (digits - 1)) + 10 ** (digits - 1));
    let bad = 0;
    const cases = [];
    for (let i = 0; i < 400; i += 1) {
      const t = rnd(13);
      cases.push([t, t], [t, String(Number(t) + 1)], [t, String(Number(t) - 1)], [t, String(Number(t) + 10 ** Math.floor(Math.random() * 12))], [t, rnd(12)], [t, rnd(14)]);
    }
    cases.push(['1793421529949', '1793421529950'], ['1793421529949', '1793421529949'], ['1793421529999', '1793421530000'], ['1799999999999', '1800000000000'], ['1999999999999', '2000000000000'], ['9999999999999', '10000000000000'], ['1000000000000', '999999999999']);
    for (const [t, x] of cases) {
      const re = new RegExp(`^(?:${unixMsAfterPattern(t)})$`);
      if (re.test(x) !== (BigInt(x) > BigInt(t))) bad += 1;
    }
    check('g5resume regex: the "later than t" pattern agrees with a numeric comparison on equal, neighbouring, shorter and longer numbers', bad === 0, `${bad} of ${cases.length} disagree`);
    const line = JSON.stringify({ event: 'accepted', unix_ms: 1793421529950, label: 'claude' });
    const same = JSON.stringify({ event: 'accepted', unix_ms: 1793421529949, label: 'claude' });
    check('g5resume regex: an event line is matched only when its unix_ms is later and its kind is the one asked for', new RegExp(eventAfterRegex('accepted', 1793421529949)).test(line) && !new RegExp(eventAfterRegex('accepted', 1793421529949)).test(same) && !new RegExp(eventAfterRegex('ended', 1793421529949)).test(line));
    const freed = JSON.stringify({ event: 'log', unix_ms: 1793421530173, line: 'oac-daemon: freed a connection on claude' });
    check('g5resume regex: the freed-connection line is matched by its text and a later unix_ms only', new RegExp(freedAfterRegex(1793421530172)).test(freed) && !new RegExp(freedAfterRegex(1793421530173)).test(freed) && !new RegExp(freedAfterRegex(1)).test(freed.replace('claude', 'codex')));
    let threw = false;
    try {
      unixMsAfterPattern('abc');
    } catch {
      threw = true;
    }
    check('g5resume regex: a non-number is refused', threw);
    const text = ['junk', JSON.stringify({ event: 'log', unix_ms: 100, line: 'oac-daemon: freed a connection on claude' }), JSON.stringify({ event: 'log', unix_ms: 250, line: 'oac-daemon: freed a connection on claude' }), 'not json {', JSON.stringify({ event: 'accepted', unix_ms: 300 })].join('\n');
    check('g5resume helpers: the daemon clock is its last event; the first freed line after t; a pane with no events has no clock', daemonClockMs(text) === 300 && firstFreedAfter(text, 100) === 250 && firstFreedAfter(text, 250) === null && daemonClockMs('nothing') === null && daemonEvents(text).length === 3);
    const pane = ['PS> echo T1; claude -p x', 'T1', '{"type":"result","session_id":"S1"}', 'DONE1', 'PS> echo T2; claude -p x --resume S1', 'T2', 'DONE2'].join('\n');
    const own = sliceFromToken(pane, 'T2');
    check('g5resume helpers: a run\'s own text starts at its own command line; an unknown token gives null; a mark is a whole line', own.startsWith('PS> echo T2') && !own.includes('S1"}') && sliceFromToken(pane, 'T9') === null && hasLine(pane, 'DONE1') && !hasLine(pane, 'DONE') && !hasLine('PS> echo DONE1', 'DONE1'));
    check('g5resume helpers: results are the type:result lines with a string session_id; every "session_id" value in a text is found', claudeResults(pane).length === 1 && sessionIdsIn('{"a":{"session_id":"x1"},"b":[{"session_id" : "x2"}]}').join() === 'x1,x2');
  }

  // --- the scenario against the double ---------------------------------------------------------------------
  const root = mkdtempSync(join(tmpdir(), 'oac-g5resume-'));
  try {
    const sub = (name) => {
      const d = join(root, name);
      mkdirSync(d, { recursive: true });
      return d;
    };

    // 1. a run as the committed record shows it
    const ok = await runScenario(sub('ok'), { sc142: 'C', distinctClaudeIds: false });
    check('g5resume happy: the three runs pass, with no finding and no error', ok.error === null && ok.out.findings.length === 0, ok.error?.stack ?? JSON.stringify(ok.out.findings));
    const names = [...ok.out.captures.keys()].sort();
    const expectNames = ['events.jsonl.txt', 'pane-claude-A-herdr.txt', 'pane-claude-B-herdr.txt', 'pane-claude-C-herdr.txt', 'pane-daemon-herdr.txt',
      ...[1, 2].map((i) => `sessions-${i}-A-during.txt`), 'sessions-3-A-after.txt', 'sessions-4-B-during.txt', 'sessions-5-B-during.txt', 'sessions-6-B-after.txt', 'sessions-7-C-during.txt', 'sessions-8-C-during.txt', 'sessions-9-C-after.txt'].sort();
    check('g5resume captures: the names the committed fixtures depend on (-herdr pane captures, sessions-<n>-<run>-during|after, events, daemon pane), nothing else', JSON.stringify(names) === JSON.stringify(expectNames), names.join(' '));
    const fixtures = readdirSync(FIXTURES).filter((f) => f !== 'README.md');
    const fixedNames = fixtures.filter((f) => !f.startsWith('sessions-'));
    check('g5resume captures: every committed fixture name is a name the scenario still writes (the pane, daemon and events names verbatim; the sessions names by their pattern)', fixedNames.every((f) => names.includes(f)) && fixtures.filter((f) => f.startsWith('sessions-')).every((f) => /^sessions-\d+-[ABC]-(during|after)\.txt$/.test(f)) && fixtures.length === 12, fixtures.join(' '));
    const obs = ok.out.records.get('observed');
    check('g5resume happy: observed keeps the committed record\'s fields per run (listedDuring, bound, harness, state, totalAfter, sleepFailed, claudeSessionIsRunA) and adds the samples', obs.runs.length === 3 && obs.runs.every((r) => r.listedDuring === 1 && r.bound === true && r.harness === 'claude' && r.state === 'online' && r.totalAfter === 0 && r.sleepFailed === false && r.claudeSessionIsRunA === true && r.unboundDuring === 0 && r.unboundAfter === 0) && obs.oacSessionIdsDistinct === true);
    check('g5resume happy: the daemon record has six accepted, six ended, no binding Finding, a clean stop, and per run two of each', ok.out.records.get('daemon').accepted === 6 && ok.out.records.get('daemon').ended === 6 && ok.out.records.get('daemon').bindingFindings.length === 0 && ok.out.records.get('daemon').stopped === true && LABELS.every((L) => ok.out.records.get('daemon').perRun[L].accepted === 2 && ok.out.records.get('daemon').perRun[L].ended === 2));
    check('g5resume happy: the daemon was told to stop once, by the main path', ok.world.log.stopsTyped === 1);
    check('g5resume happy: run B was started with --resume of run A\'s Claude Code id, run C with --continue, A with neither', /--resume 3f0c1e52-9a77-4c1d-8d2e-5b6a7c8d9e01/.test(ok.world.log.claudeCommands[1]) && /--continue/.test(ok.world.log.claudeCommands[2]) && !/--resume|--continue/.test(ok.world.log.claudeCommands[0]));
    check('g5resume happy: no raw OAC or Claude session id is in a capture, a record or a finding', rawLeaks(ok, {}).length === 0, rawLeaks(ok, {}).join());
    check('g5resume happy: placeholders as the committed fixtures have them (<OAC_SESSION_A> .. C, <CLAUDE_SESSION_1> for the shared Claude id)', [...ok.out.captures.get('sessions-2-A-during.txt').matchAll(/<OAC_SESSION_[A-C]>/g)].length === 1 && ok.out.captures.get('sessions-2-A-during.txt').includes('<OAC_SESSION_A>') && ok.out.captures.get('sessions-5-B-during.txt').includes('<OAC_SESSION_B>') && ok.out.captures.get('pane-claude-B-herdr.txt').includes('<CLAUDE_SESSION_1>') && !ok.out.captures.get('pane-claude-B-herdr.txt').includes('<CLAUDE_SESSION_2>'));

    // 2. sampling: more than once while alive, on events
    check('g5resume sampling: each run has at least two session lists taken while it was alive (after its first accepted event, then after the hook handler\'s freed line), then one after it exited', obs.runs.every((r) => r.aliveSamples >= 2 && r.samples.map((s) => s.trigger).join() === 'accepted,hook-freed' && r.samples.every((s) => s.alive)), JSON.stringify(obs.runs[0].samples));
    check('g5resume sampling: the first sample of a run is taken before the session is bound (0 listed, an unbound attachment), the second once it is (1 listed, none unbound)', obs.runs.every((r) => r.samples[0].total === 0 && r.samples[0].unbound === 1 && r.samples[1].total === 1 && r.samples[1].unbound === 0));
    const dWaits = ok.world.log.waits.filter((w) => w.role === 'daemon').map((w) => w.regex);
    check('g5resume sampling: before each sample the scenario waited on the daemon pane for an event of the run (accepted, then freed), with the daemon\'s own clock as the cutoff, and again for the shim\'s freed line', dWaits.filter((r) => /"event":"accepted"/.test(r)).length === 3 && dWaits.filter((r) => /"event":"log".*freed a connection on claude/.test(r)).length === 6);
    check('g5resume sampling: the scenario has no timer: no sleep, setTimeout or setInterval, and no pollAttempts', !/\bsleep\s*\(|setTimeout|setInterval|pollAttempts|Atomics\.wait/.test(code) && !('pollAttempts' in scenario.defaults.params));
    check('g5resume sampling: a whole run through the double takes no wall-clock wait (the old 3 s and 2 s sleeps are gone)', ok.ms < 2500, `${ok.ms} ms`);

    const late = await runScenario(sub('late'), { lateFreed: true });
    check('g5resume sampling: the after-exit list is taken on the shim freed line, not on the done marker: with the freed line after the marker the session is still gone when listed', late.error === null && late.out.records.get('observed').runs.every((r) => r.totalAfter === 0), late.error?.stack);

    // 3. per-run pane slicing
    const sliceB = ok.out.captures.get('pane-claude-B-herdr.txt');
    const sliceC = ok.out.captures.get('pane-claude-C-herdr.txt');
    check('g5resume slicing: run B\'s capture holds only its own command line and result (no run A line, one result), and run C\'s only its own', !/G5R-A-/.test(sliceB) && (sliceB.match(/"type":"result"/g) ?? []).length === 1 && /--resume/.test(sliceB) && !/G5R-B-/.test(sliceC) && (sliceC.match(/"type":"result"/g) ?? []).length === 1 && /--continue/.test(sliceC) && !/--resume/.test(sliceC));
    const noRes = await runScenario(sub('nores'), { noResult: ['B'] });
    check('g5resume slicing: a run with no result of its own does not take an earlier run\'s Claude id (null, not run A\'s), and the leg fails on it', noRes.error && /expectations not met/.test(noRes.error.message) && noRes.out.records.get('observed').runs[1].claudeSessionIsRunA === null && noRes.out.findings.some((f) => /^run B: no Claude Code result with a session id/.test(f)) && !noRes.out.findings.some((f) => /^run [AC]: no Claude Code result/.test(f)), noRes.error?.message + JSON.stringify(noRes.out.findings));
    const slp = await runScenario(sub('sleepfailed'), { sleepFailed: ['A'] });
    check('g5resume slicing: the "missing operand" check reads a run\'s own text: a failed hold in run A is not charged to B and C', slp.error && slp.out.findings.filter((f) => /held Bash call failed/.test(f)).join() === 'run A: the held Bash call failed; the session was not held', JSON.stringify(slp.out.findings));
    const dist = await runScenario(sub('distinct'), { distinctClaudeIds: true });
    check('g5resume slicing: with a different Claude id per run each capture holds only its own id; ids are numbered in order of appearance; run B/C report claudeSessionIsRunA false', dist.error === null && dist.out.captures.get('pane-claude-A-herdr.txt').includes('<CLAUDE_SESSION_1>') && dist.out.captures.get('pane-claude-B-herdr.txt').includes('<CLAUDE_SESSION_2>') && /"session_id":"<CLAUDE_SESSION_2>"/.test(dist.out.captures.get('pane-claude-B-herdr.txt')) && !dist.out.captures.get('pane-claude-B-herdr.txt').includes('<CLAUDE_SESSION_3>') && dist.out.captures.get('pane-claude-C-herdr.txt').includes('<CLAUDE_SESSION_3>') && dist.out.records.get('observed').runs.map((r) => r.claudeSessionIsRunA).join() === 'true,false,false' && rawLeaks(dist, { distinctClaudeIds: true }).length === 0, dist.error?.stack);

    // 4. early redaction and per-step captures on failure paths
    const extra = await runScenario(sub('extra'), { extraSession: 'B' });
    const lits = extra.out.literals.map(([v]) => v);
    check('g5resume redaction: every listed session_id is redacted, also when sessions_total is more than one (run B listed two), under distinct placeholders', lits.includes(oacIdOf('B')) && lits.includes(extraIdOf('B')) && extra.out.literals.find(([v]) => v === extraIdOf('B'))[1] === '<OAC_SESSION_B_2>' && extra.out.literals.find(([v]) => v === oacIdOf('B'))[1] === '<OAC_SESSION_B>' && rawLeaks(extra, {}).length === 0 && extra.error && extra.out.findings.some((f) => /^run B: oac sessions did not list exactly one bound session while it was alive \(listed 2\)/.test(f)), JSON.stringify(extra.out.findings));
    check('g5resume redaction: a literal is registered once per value', new Set(lits).size === lits.length);
    const missing = await runScenario(sub('missing'), { doneMissing: 'B', distinctClaudeIds: true });
    const mnames = [...missing.out.captures.keys()];
    check('g5resume failure path: a run that never finishes ends the leg with the wait error, not a misleading one', missing.error instanceof Timeout, String(missing.error));
    check('g5resume failure path: the sessions-* captures taken before the failure are kept (run A\'s three, run B\'s two during), as are run A\'s pane capture and the not-done capture of B, and the daemon evidence', ['sessions-1-A-during.txt', 'sessions-2-A-during.txt', 'sessions-3-A-after.txt', 'sessions-4-B-during.txt', 'sessions-5-B-during.txt', 'pane-claude-A-herdr.txt', 'pane-claude-B-not-done-herdr.txt', 'pane-daemon-herdr.txt', 'events.jsonl.txt'].every((n) => mnames.includes(n)) && !mnames.includes('pane-claude-B-herdr.txt'), mnames.join(' '));
    check('g5resume failure path: ids that appear only on the failure path (run B\'s own Claude id in the not-done capture, its OAC id in the sessions captures) are redacted in what is kept', rawLeaks(missing, { distinctClaudeIds: true }).length === 0 && missing.out.captures.get('pane-claude-B-not-done-herdr.txt').includes('<CLAUDE_SESSION_2>') && !missing.out.captures.get('pane-claude-B-not-done-herdr.txt').includes('G5R-A-'), rawLeaks(missing, { distinctClaudeIds: true }).join());
    check('g5resume failure path: the observed record is kept as incomplete', missing.out.records.get('observed')?.incomplete === true && missing.out.records.get('observed').runs.length === 2);

    // 5. cleanup: the daemon is stopped once, and never when it already stopped
    check('g5resume cleanup: after a timeout the error path still attempts the stop once; the driver refuses input then, and the refusal is swallowed', missing.world.log.stopsTyped === 0 && missing.world.log.stopRefused === 1, `typed ${missing.world.log.stopsTyped}, refused ${missing.world.log.stopRefused}`);
    const noA = await runScenario(sub('noa'), { noResult: ['A'] });
    check('g5resume cleanup: on a mid-run failure that is not a timeout (run A has no result) the error path types stop once, and the daemon stops', noA.error && /cannot resume/.test(noA.error.message) && noA.world.log.stopsTyped === 1 && noA.out.captures.has('pane-daemon-herdr.txt'), `${noA.error?.message} typed ${noA.world.log.stopsTyped}`);
    const unmet = await runScenario(sub('unmet'), { state: 'offline' });
    check('g5resume cleanup: when the criteria fail after the main path stopped the daemon, the error path does not type stop again', unmet.error && /expectations not met/.test(unmet.error.message) && unmet.world.log.stopsTyped === 1 && unmet.out.captures.has('pane-daemon-herdr.txt') && unmet.out.captures.has('events.jsonl.txt'), `typed ${unmet.world.log.stopsTyped}`);
    const self = await runScenario(sub('self'), { selfStop: true });
    check('g5resume cleanup: a daemon that already stopped on its own is not sent stop (the pane shows the stopped event), and the leg still passes', self.error === null && self.world.log.stopsTyped === 0 && self.out.records.get('daemon').stopped === true, self.error?.stack);
    const selfFail = await runScenario(sub('selffail'), { selfStop: true, state: 'offline' });
    check('g5resume cleanup: a daemon that stopped on its own and then an unmet criterion: still no stop typed, and the original error is the one thrown', selfFail.error && /expectations not met/.test(selfFail.error.message) && selfFail.world.log.stopsTyped === 0);
    const never = await runScenario(sub('never'), { daemonNeverReady: true });
    check('g5resume cleanup: the daemon-not-ready path keeps its capture, rethrows the wait error, and still tries to stop the daemon (the driver refuses input after a timeout; the attempt is made and its refusal swallowed)', never.error instanceof Timeout && never.out.captures.has('pane-daemon-not-ready-herdr.txt') && never.world.log.stopRefused === 1 && never.world.log.stopsTyped === 0 && never.out.records.get('observed') === undefined, `${never.error} refused ${never.world.log.stopRefused}`);
    const gone = await runScenario(sub('gone'), { readyWaitError: true });
    check('g5resume cleanup: when the ready wait fails for a reason other than a timeout, the daemon that is running is stopped (stop typed once, the stopped event seen)', gone.error && /pane is gone/.test(gone.error.message) && gone.world.log.stopsTyped === 1 && gone.out.captures.has('pane-daemon-not-ready-herdr.txt'), `${gone.error} typed ${gone.world.log.stopsTyped}`);

    // 6. the stricter pass criteria, one at a time
    const must = async (label, opts, finding, extraCheck = () => true) => {
      const r = await runScenario(sub(label.replace(/\W+/g, '-')), opts);
      check(`g5resume criteria: ${label}`, r.error && /expectations not met/.test(r.error.message) && r.out.findings.some((f) => finding.test(f)) && extraCheck(r), `${r.error?.message} ${JSON.stringify(r.out.findings)}`);
    };
    await must('a state other than online fails the run', { state: 'offline' }, /^run A: the bound session reported harness "claude" and state "offline"/);
    await must('a harness other than claude fails the run', { harness: 'codex' }, /^run A: the bound session reported harness "codex" and state "online"/);
    await must('unbound_attachments above 0 once bound fails the run', { unboundDuring: 1 }, /^run A: oac sessions reported unbound_attachments 1 once the session was bound/);
    await must('unbound_attachments above 0 after the run exited fails the run', { unboundAfter: 2 }, /^run A: oac sessions reported unbound_attachments 2 after it exited/);
    await must('a session still listed after the run exited fails the run', { staleAfter: 'B' }, /^run B: 1 session\(s\) still listed after it exited/);
    await must('five accepted connections instead of six fail the run', { dropAccepted: 'C' }, /^the daemon accepted 5 and ended 6 connections \(expected 6 of each/);
    await must('a binding Finding line in the daemon log fails the run', { findingLine: 'B' }, /^the daemon logged 1 binding Finding line\(s\)/, (r) => r.out.records.get('daemon').bindingFindings.length === 1);
    await must('a run that ended before its second sample was taken (one sample alive) fails the run', { markEarly: 'B' }, /^run B: 1 of 2 session-list sample\(s\) were taken while it was alive \(at least 2 are required\)/);
    await must('two runs bound to one OAC session id fail the run', { sameOacId: true }, /did not get three distinct OAC session ids/);
    check('g5resume criteria: the number of connections expected is two per run', EXPECTED_CONNECTIONS === 6);

    // 7. SC-ID-142 diagnostics: recorded per run, never failed on
    const dg = ok.out.records.get('daemon');
    check('g5resume SC-ID-142: the diagnostic in run C\'s window is recorded under C only, among the binding lines, and the leg passes', dg.perRun.C.sc142.length === 1 && /SC-ID-142/.test(dg.perRun.C.sc142[0]) && dg.perRun.A.sc142.length === 0 && dg.perRun.B.sc142.length === 0 && dg.bindingLines.length === 1 && ok.error === null);
    const dgB = await runScenario(sub('sc142b'), { sc142: 'B' });
    check('g5resume SC-ID-142: a diagnostic in run B is recorded under B, and does not fail the leg', dgB.error === null && dgB.out.records.get('daemon').perRun.B.sc142.length === 1 && dgB.out.records.get('daemon').perRun.C.sc142.length === 0 && !dgB.out.findings.some((f) => /SC-ID-142/.test(f)));

    // 8. the other pane shell types a different exit variable, and the leg runs the same
    const ps = await runScenario(sub('ps'), {}, {}, 'powershell');
    check('g5resume shell: under the PowerShell pane shell the leg passes the same way ($LASTEXITCODE)', ps.error === null && ps.out.findings.length === 0 && ps.world.log.opsCommands === 9, ps.error?.stack);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }

  // --- the judging functions on hand-built observations -------------------------------------------------
  {
    const list = (id, extra = {}) => ({ oac_cli: 1, sessions: id ? [{ harness: 'claude', session_id: id, state: 'online', ...extra }] : [], sessions_total: id ? 1 : 0, unbound: [{ harness: 'claude', unbound_attachments: 0 }] });
    const mk = (L, id) => ({
      label: L, t0: 1, claudeSessionId: 'c', sleepFailed: false,
      samples: [{ n: 1, trigger: 'accepted', alive: true, code: 0, o: list(null) }, { n: 2, trigger: 'hook-freed', alive: true, code: 0, o: list(id) }],
      after: { n: 3, code: 0, o: list(null) },
    });
    const runs = [mk('A', 'a'), mk('B', 'b'), mk('C', 'c')];
    const dm = { accepted: 6, ended: 6, bindingFindings: [], stopped: true };
    check('g5resume judge: three clean runs and a clean daemon give no problem', judgeG5(runs, dm).length === 0);
    const mutated = (fn) => {
      const r = structuredClone(runs);
      fn(r);
      return judgeG5(r, dm);
    };
    check('g5resume judge: an unbound list that is missing (not an array) counts as unmet, never as 0', mutated((r) => delete r[0].samples[1].o.unbound).some((p) => /unbound_attachments null/.test(p)));
    check('g5resume judge: a later alive sample listing a different id fails the run', mutated((r) => { r[1].samples.push({ n: 9, trigger: 'extra', alive: true, code: 0, o: list('zzz') }); }).some((p) => /^run B: the samples taken while it was alive listed 1 other session id/.test(p)));
    check('g5resume judge: a sample taken after the run ended (alive false) is not charged to the run', mutated((r) => { r[1].samples.push({ n: 9, trigger: 'late', alive: false, code: 0, o: list('zzz') }); }).length === 0);
    check('g5resume judge: the Claude ids and their sameness are not judged', mutated((r) => { r[1].claudeSessionId = 'other'; }).length === 0);
    check('g5resume judge: a clean stop is required', judgeG5(runs, { ...dm, stopped: false }).some((p) => /clean stop/.test(p)));
    check('g5resume summary: a run with no samples is unbound, listed 0, nothing alive', (() => {
      const s = summarizeRun({ label: 'A', samples: [], after: null });
      return s.listedDuring === 0 && s.bound === false && s.aliveSamples === 0 && s.oacSessionId === null && s.totalAfter === null;
    })());
    const sm = daemonSummary(
      [
        { event: 'ready', unix_ms: 10 }, { event: 'accepted', unix_ms: 120 }, { event: 'accepted', unix_ms: 121 }, { event: 'ended', unix_ms: 125 },
        { event: 'log', unix_ms: 130, line: 'oac-daemon: binding Diagnostic SC-ID-142 Bound attachment=None session=None' },
        { event: 'accepted', unix_ms: 220 }, { event: 'ended', unix_ms: 225 }, { event: 'stopped', unix_ms: 300 },
      ].map((e) => JSON.stringify(e)).join('\n'),
      [{ label: 'A', t0: 100 }, { label: 'B', t0: 200 }],
    );
    check('g5resume summary: connections and SC-ID-142 diagnostics are assigned to a run by the daemon\'s clock at the run\'s start', sm.accepted === 3 && sm.ended === 2 && sm.perRun.A.accepted === 2 && sm.perRun.A.sc142.length === 1 && sm.perRun.B.accepted === 1 && sm.perRun.B.sc142.length === 0 && sm.stopped === true && sm.bindingFindings.length === 0);
  }

  // --- binding after the hook's freed line, sources, redaction of ids by shape, the not-done and not-ready evidence ---
  {
    const root2 = mkdtempSync(join(tmpdir(), 'oac-g5resume2-'));
    const sub2 = (name) => {
      const d = join(root2, name);
      mkdirSync(d, { recursive: true });
      return d;
    };
    try {
      const lateB = await runScenario(sub2('late-bind'), { lateBind: 1 });
      const lo = lateB.out.records.get('observed');
      check('g5resume bind: when the bind decision comes one step after the hook freed line, the leg still passes: the hook-freed sample lists nothing, one bind-retry sample lists the session, and that one is judged', lateB.error === null && lateB.out.findings.length === 0 && lo.runs.every((r) => r.samples.map((x) => x.trigger).join() === 'accepted,hook-freed,bind-retry-1' && r.samples[1].total === 0 && r.samples[2].total === 1 && r.listedDuring === 1 && r.bound === true && r.aliveSamples === 3), lateB.error?.stack ?? JSON.stringify(lo?.runs[0].samples));
      const lateTwo = await runScenario(sub2('late-bind-3'), { lateBind: 3 });
      check('g5resume bind: a bind three samples late is still found (retries stop at the first sample that lists a session)', lateTwo.error === null && lateTwo.out.records.get('observed').runs.every((r) => r.samples.map((x) => x.trigger).join() === 'accepted,hook-freed,bind-retry-1,bind-retry-2,bind-retry-3'), lateTwo.error?.stack);
      const never = await runScenario(sub2('never-bind'), { neverBind: true });
      check('g5resume bind: a session that is never listed fails the run after a bounded number of retries (' + MAX_BIND_RETRIES + ' extra samples per run), with the listed-0 finding', never.error && /expectations not met/.test(never.error.message) && never.world.log.opsCommands === 3 * (2 + MAX_BIND_RETRIES + 1) && never.out.findings.some((f) => /^run A: oac sessions did not list exactly one bound session while it was alive \(listed 0\)/.test(f)), 'ops ' + never.world.log.opsCommands + ' ' + never.error);
      const promptly = await runScenario(sub2('prompt-bind'), {});
      check('g5resume bind: a session bound by the hook-freed sample takes no retry sample', promptly.out.records.get('observed').runs.every((r) => r.samples.length === 2));

      const okSrc = await runScenario(sub2('sources'), {});
      const dw = okSrc.world.log.waits.filter((w) => w.role === 'daemon');
      check('g5resume sources: every wait on the daemon pane (ready, accepted, freed, stopped) asks for recent-unwrapped, so a long event line is not cut by the pane width', dw.length >= 10 && dw.every((w) => w.source === 'recent-unwrapped'), JSON.stringify(dw.filter((w) => w.source !== 'recent-unwrapped')));

      const shape = await runScenario(sub2('shape'), { proseId: 'B', logId: 'C', distinctClaudeIds: true });
      const everything = [...shape.out.captures.values()].join('\n') + JSON.stringify([...shape.out.records]) + shape.out.findings.join('\n');
      check('g5resume redaction by shape: an OAC session id that only appears in the prose of Claude or in a daemon binding line (never listed by oac sessions) is redacted before the capture and the record', shape.error === null && !everything.includes(PROSE_ID) && !everything.includes(LOG_ID) && shape.out.literals.some(([v]) => v === PROSE_ID) && shape.out.literals.some(([v]) => v === LOG_ID) && rawLeaks(shape, { distinctClaudeIds: true }).length === 0, shape.error?.stack ?? rawLeaks(shape, { distinctClaudeIds: true }).join());
      check('g5resume redaction by shape: the shape is the one of core/src/ids.rs (26 characters, first 0-7, Crockford base32), as a whole token', oacIdsIn('x ' + PROSE_ID + ' y "' + LOG_ID + '"').join() === PROSE_ID + ',' + LOG_ID && oacIdsIn('8' + 'p'.repeat(25)).length === 0 && oacIdsIn('5' + 'p'.repeat(26)).length === 0 && oacIdsIn('5' + 'p'.repeat(24) + 'i').length === 0 && oacIdsIn('5' + 'P'.repeat(25)).length === 0 && oacIdsIn('a5' + 'p'.repeat(25)).length === 0);

      const hidden = await runScenario(sub2('hidden'), { doneMissing: 'B', hideBegin: 'B', distinctClaudeIds: true });
      const nd = hidden.out.captures.get('pane-claude-B-not-done-herdr.txt') ?? '';
      check('g5resume failure path: with the begin marker gone from the pane the not-done capture is the whole pane (not empty), its ids learned first', hidden.error instanceof Timeout && nd.length > 0 && nd.includes('<CLAUDE_SESSION_2>') && rawLeaks(hidden, { distinctClaudeIds: true }).length === 0, rawLeaks(hidden, { distinctClaudeIds: true }).join());
      const nr = await runScenario(sub2('notready'), { daemonNeverReady: true });
      const gn = await runScenario(sub2('gone'), { readyWaitError: true });
      check('g5resume failure path: on the not-ready path the daemon pane is captured once, as pane-daemon-not-ready-herdr.txt, with no duplicate pane-daemon-herdr.txt', nr.out.captures.has('pane-daemon-not-ready-herdr.txt') && !nr.out.captures.has('pane-daemon-herdr.txt') && gn.out.captures.has('pane-daemon-not-ready-herdr.txt') && !gn.out.captures.has('pane-daemon-herdr.txt'));
    } finally {
      rmSync(root2, { recursive: true, force: true });
    }
    const tie = daemonSummary(
      [{ event: 'accepted', unix_ms: 120 }, { event: 'ended', unix_ms: 200 }, { event: 'ended', unix_ms: 201 }, { event: 'accepted', unix_ms: 300 }].map((e) => JSON.stringify(e)).join('\n'),
      [{ label: 'B', t0: 100 }, { label: 'C', t0: 200 }],
    );
    check('g5resume summary: an event at exactly the t0 of a run (the daemon last event before it) is the previous run, not the next one', tie.perRun.B.ended === 1 && tie.perRun.C.ended === 1 && tie.perRun.B.accepted === 1 && tie.perRun.C.accepted === 1);
    {
      // The committed run's own events, with each run's t0 taken as the scenario takes it: the daemon's last event before the run.
      const text = readFileSync(join(FIXTURES, 'events.jsonl.txt'), 'utf8');
      const evs = daemonEvents(text);
      const accepted = evs.map((e, i) => (e.event === 'accepted' ? i : -1)).filter((i) => i >= 0);
      const t0s = [0, 2, 4].map((k) => Math.max(...evs.slice(0, accepted[k]).map((e) => e.unix_ms)));
      const real = daemonSummary(text, ['A', 'B', 'C'].map((label, i) => ({ label, t0: t0s[i] })));
      check('g5resume summary: on the events of the committed run each run owns two accepted and two ended connections, and the one SC-ID-142 line is run C', ['A', 'B', 'C'].every((L) => real.perRun[L].accepted === 2 && real.perRun[L].ended === 2) && real.perRun.C.sc142.length === 1 && real.perRun.A.sc142.length === 0 && real.perRun.B.sc142.length === 0 && real.accepted === 6 && real.ended === 6 && real.bindingFindings.length === 0 && real.stopped === true, JSON.stringify(real.perRun));
    }
    const ts = (arr) => arr.map((x, i) => ({ n: i, trigger: 't' + i, alive: true, code: 0, o: x }));
    const one = { oac_cli: 1, sessions: [{ harness: 'claude', session_id: 'a', state: 'online' }], sessions_total: 1, unbound: [] };
    const none = { oac_cli: 1, sessions: [], sessions_total: 0, unbound: [] };
    const sm2 = summarizeRun({ label: 'A', samples: ts([none, none, one, one]), after: null });
    check('g5resume summary: the bound sample is the first alive one that lists a session (not a fixed trigger), and a sample listing one after the run ended is not it', sm2.oacSessionId === 'a' && sm2.listedDuring === 1 && summarizeRun({ label: 'A', samples: [{ ...ts([one])[0], alive: false }], after: null }).listedDuring === 0);
  }

  // --- the source ----------------------------------------------------------------------------------------------
  check('g5resume source: the stop is typed only through stopDaemon, which is guarded by the stopped flag', (code.match(/paneRun\(wsA\.paneId, 'stop'\)/g) ?? []).length === 1 && /if \(stopped \|\| stopTried\) return;/.test(code));
  check('g5resume source: the typed command for each run starts with its own begin marker echo (the slice anchor)', /`echo \$\{begin\}; \$\{claude\}; echo \$\{mark\}`/.test(code));
}
