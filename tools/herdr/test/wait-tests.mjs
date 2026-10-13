// Unit checks for herdr waits, settles and settled reads (#253, #246). In-process and
// cross-platform: the fake-herdr checks spawn test/fake-herdr.mjs with node only (no pane
// shell), so they run on Windows too.
//
// What they pin:
//   - herdr v0.9.1 reports an agent's state as AgentInfo.agent_status; a response without one
//     (or the `state` field the driver used to read) is not a state: null, never settled;
//   - a settle whose wait returns null ends the run NOT RUN with a finding;
//   - herdr's rule for a turn having begun (src/api/wait.rs L249-275, L515-520): an OBSERVED
//     working or blocked state past the baseline. A state change past the baseline without
//     one (unknown@6 then idle@7) is not a turn; a push's turn still running holds the next
//     prompt back; a prompt is never typed into a running turn;
//   - `begun` (wire evidence a turn started) never lets `unknown` settle; only `done` does;
//   - an after-delivery read waits for idle first, retries after agent_not_idle, and gives up
//     NOT RUN after three refusals; nothing is re-sent;
//   - fake-herdr imitates herdr's agent_not_idle refusal, the agent_status shape and
//     `agent prompt --wait`'s activity gate.

import { appendFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { HerdrSession, NotRunError, agentStatusOf, stateChangeSeqOf } from '../lib/herdr.mjs';
import { makeAgent, recordWaitState, turnFloor, pastFloor, refuseRunningTurn } from '../lib/gate-common.mjs';
import { threadIdleOnWire } from '../lib/g5.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const FAKE = join(HERE, 'fake-herdr.mjs');

const rejectsWith = async (fn, re) => {
  try {
    await fn();
    return false;
  } catch (err) {
    return err instanceof NotRunError && (!re || re.test(err.message));
  }
};

// A scripted stand-in for HerdrSession. Settle waits, `agent get` baselines, the background
// activity watch, full reads and prompts each answer from their own script; every call is
// logged in order.
function stubHerdr({ waits = [], thenWait = { state: 'idle', stateChangeSeq: 5 }, gets = [], watch = { state: 'working', stateChangeSeq: 6 }, reads = [], prompted = { state: 'idle', stateChangeSeq: 7 } } = {}) {
  const log = [];
  const commands = [];
  const entry = (kind) => {
    const e = { seq: commands.length + 1, kind, startedAt: new Date().toISOString(), endedAt: new Date().toISOString(), durationMs: 1 };
    commands.push(e);
    log.push(kind);
    return e;
  };
  return {
    log,
    commands,
    abortSignal: null,
    async agentWait(_name, { background = false } = {}) {
      if (background) {
        if (watch instanceof Error) {
          entry('watch:timeout');
          throw watch;
        }
        return { entry: entry('watch'), state: watch.state, stateChangeSeq: watch.stateChangeSeq };
      }
      const w = waits.length ? waits.shift() : thenWait;
      if (w instanceof Error) {
        entry('wait:timeout');
        throw w;
      }
      return { entry: entry('wait'), json: null, state: w.state, stateChangeSeq: w.stateChangeSeq ?? null };
    },
    async agentGetState() {
      const s = gets.length ? gets.shift() : { state: 'idle', stateChangeSeq: 5 };
      return { entry: entry('get'), state: s.state, stateChangeSeq: s.stateChangeSeq };
    },
    async agentRead(_name, { source = 'visible' } = {}) {
      entry(`read:${source}`);
      return 'idle screen';
    },
    async agentReadResult(_name, { source }) {
      const r = reads.length ? reads.shift() : { errorCode: null };
      const e = entry(`full-read:${source}${r.errorCode ? `:${r.errorCode}` : ''}`);
      return { text: r.errorCode ? null : 'full history', errorCode: r.errorCode, entry: e };
    },
    async agentPrompt(_name, _text, { wait = null } = {}) {
      const e = entry(wait ? 'prompt --wait' : 'prompt');
      return wait ? { entry: e, state: prompted.state, stateChangeSeq: prompted.stateChangeSeq } : { entry: e, state: 'idle', stateChangeSeq: 5 };
    },
  };
}

function agentWith(herdr, { findings = [] } = {}) {
  const g = { herdrStates: [], dialogs: [] };
  const ctx = { herdr, remainingMs: () => 60000, finding: (f) => findings.push(f), sessionName: 'unit' };
  const stop = (reason) => {
    throw new NotRunError(reason);
  };
  const params = { settleMs: 0, pollMs: 1, maxDialogs: 3, humanAcceptTimeoutMs: 1000, turnTimeoutMs: 5000 };
  const agent = makeAgent({ ctx, g, name: 'unit', label: 'claude', classify: (t) => ({ busy: /BUSY/.test(t), dialog: null }), dialogKinds: {}, driverMayAccept: () => null, accept: 'driver', num: (k) => params[k], stop });
  return { agent, g, findings };
}

// A fake-herdr session with one agent whose harness-double files the test writes itself.
function fakeSession(mode, { state = 'idle', seq = 3, at = 0 } = {}) {
  const root = mkdtempSync(join(tmpdir(), 'oac-wait-unit-'));
  const name = 'unit-sess';
  const sdir = join(root, 'sessions', name);
  const adir = join(sdir, 'agents', 'a.d');
  mkdirSync(join(sdir, 'panes'), { recursive: true });
  mkdirSync(adir, { recursive: true });
  writeFileSync(join(sdir, 'server.json'), JSON.stringify({ pid: process.pid }));
  writeFileSync(join(sdir, 'panes', 'w1_p1.json'), JSON.stringify({ id: 'w1:p1', cwd: root, pid: process.pid, env: {} }));
  writeFileSync(join(sdir, 'panes', 'w1_p1.buf'), 'history line\n');
  writeFileSync(join(sdir, 'agents', 'a.json'), JSON.stringify({ pane: 'w1:p1', pid: null, dir: adir }));
  writeFileSync(join(adir, 'screen.txt'), 'visible screen\n');
  const setAgent = (s, n, t = 0) => {
    // As the harness doubles do: the transition logged, then one atomic snapshot.
    appendFileSync(join(adir, 'state-log'), `${n} ${s}\n`);
    writeFileSync(join(adir, 'state'), s);
    writeFileSync(join(adir, 'state-seq'), `${n} ${t}`);
    writeFileSync(join(adir, 'status.tmp'), `${s} ${n} ${t}`);
    renameSync(join(adir, 'status.tmp'), join(adir, 'status'));
  };
  // A transition herdr's event stream carries but no snapshot ever shows (shorter than any poll).
  const logOnly = (s, n) => appendFileSync(join(adir, 'state-log'), `${n} ${s}\n`);
  // Resolves once fake-herdr's `agent wait` has taken its event position (waits.log).
  const waitArmed = async (count) => {
    for (let i = 0; i < 400; i++) {
      const f = join(root, 'waits.log');
      if (existsSync(f) && readFileSync(f, 'utf8').split('\n').filter(Boolean).length >= count) return true;
      await new Promise((r) => setTimeout(r, 25));
    }
    return false;
  };
  setAgent(state, seq, at);
  const herdr = new HerdrSession({ herdrCmd: [process.execPath, FAKE], sessionName: name, env: { ...process.env, FAKE_HERDR_STATE: root, FAKE_HERDR_MODE: mode }, cwd: root, timebox: { remainingMs: () => 60000 }, commands: [], defaultDeadlineMs: 15000 });
  // Each fake-herdr call is a child process run with cwd: root. On Windows a child that has
  // just exited (or an AV scanner) can still hold root open briefly, and the delete fails
  // EPERM; Node's own retries absorb that (as in g4/g5-tests), and a directory that still
  // cannot be removed after them throws, failing the self-test.
  return { herdr, setAgent, logOnly, waitArmed, cleanup: () => rmSync(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 }) };
}

export async function waitUnit(check) {
  // --- parsing herdr's answer ---------------------------------------------------------------
  const info = (agent) => ({ id: 'cli', result: { type: 'agent_info', agent } });
  check('#253 agentStatusOf: herdr v0.9.1 reports the state as agent_status, with state_change_seq', agentStatusOf(info({ agent_status: 'done', state_change_seq: 4 })) === 'done' && stateChangeSeqOf(info({ agent_status: 'done', state_change_seq: 4 })) === 4);
  check('#253 agentStatusOf: a `state` field (what the driver used to read), an undocumented value, or no answer is null, not a state', agentStatusOf(info({ state: 'idle' })) === null && agentStatusOf(info({ agent_status: 'sleeping' })) === null && agentStatusOf(null) === null && stateChangeSeqOf(info({ state_change_seq: -1 })) === null && stateChangeSeqOf(info({})) === null);

  // --- fail closed on a wait that cannot establish the state --------------------------------
  {
    const findings = [];
    const g = { herdrStates: [] };
    const ok = await rejectsWith(() => recordWaitState({ g, agent: 'codex', context: 'x', w: { entry: { seq: 7 }, state: null }, ctx: { finding: (f) => findings.push(f) }, stop: (r) => { throw new NotRunError(r); } }), /did not report the agent's state/);
    check('#253 recordWaitState: a wait returning null is recorded, a finding, and NOT RUN (never settled)', ok && g.herdrStates.length === 1 && g.herdrStates[0].state === null && findings.some((f) => /#253/.test(f) && /#7/.test(f)));
  }
  {
    const h = stubHerdr({ waits: [{ state: null }] });
    const { agent, findings } = agentWith(h);
    check('#253 settle: a wait whose answer carries no agent_status ends the settle NOT RUN; no read decides it', (await rejectsWith(() => agent.settle('startup', 5000))) && findings.some((f) => /#253/.test(f)) && h.log.join() === 'wait');
  }

  // --- unknown: only `done` (the turn is over on the wire) lets it settle; `begun` never -------
  {
    const unknownAlways = { thenWait: { state: 'unknown', stateChangeSeq: 1 } };
    const { agent } = agentWith(stubHerdr(unknownAlways));
    check('#253 settle: `unknown` is not settled without wire evidence that the turn is over (times out NOT RUN)', await rejectsWith(() => agent.settle('turn', 60), /did not settle/));
    const { agent: a2 } = agentWith(stubHerdr(unknownAlways));
    check('#253 settle: `begun` (a tool call on the wire: the turn started) does not let `unknown` settle', await rejectsWith(() => a2.settle('turn', 60, { begun: async () => 'tools/call at line 9' }), /did not settle/));
    const r = await agentWith(stubHerdr({ waits: [{ state: 'unknown', stateChangeSeq: 1 }] })).agent.settle('turn', 5000, { done: async () => 'turn T completed on the wire (thread/turns/list)' });
    check('#253 settle: `unknown` with `done` (the turn over on the wire) settles, the evidence recorded', r.settled?.state === 'unknown' && r.settled?.doneEvidence === 'turn T completed on the wire (thread/turns/list)');
  }

  // --- herdr's rule: a turn began only on an OBSERVED working/blocked past the baseline --------
  {
    // The reviewer's case: after a baseline at 5, unknown@6 then idle@7 with no working seen.
    const h = stubHerdr({ gets: [{ state: 'idle', stateChangeSeq: 5 }], watch: new NotRunError('herdr agent wait (wait) timed out (herdr timeout); run ends NOT RUN, nothing re-submitted'), waits: [{ state: 'unknown', stateChangeSeq: 6 }, { state: 'idle', stateChangeSeq: 7 }] });
    const { agent, findings } = agentWith(h);
    const w = await agent.watch('C1-push');
    check('#253 settle(since watch): unknown@6 then idle@7 past the baseline, with no working or blocked observed, does NOT settle (NOT RUN, finding)', (await rejectsWith(() => agent.settle('C1-turn', 5000, { since: w }))) && findings.some((f) => /no working or blocked state/.test(f)) && !h.log.includes('wait'), h.log.join());
    const findings2 = [];
    const g2 = { herdrStates: [] };
    const notActivity = { kind: 'watch', base: { seq: 1, state: 'idle', stateChangeSeq: 5 }, running: false, promise: Promise.resolve(), result: { entry: { seq: 2 }, state: 'idle', stateChangeSeq: 7 }, error: null };
    check('#253 turnFloor: an activity answer that is not working/blocked past the baseline is not a turn (NOT RUN)', await rejectsWith(() => turnFloor({ since: notActivity, g: g2, context: 'x', ctx: { finding: (f) => findings2.push(f) }, stop: (r) => { throw new NotRunError(r); } }), /is not activity past the baseline/));
    check('#253 turnFloor: a plain prompt record (no herdr-observed start) is refused as `since`', await rejectsWith(() => turnFloor({ since: { seq: 3, stateChangeSeq: 5 }, g: g2, context: 'x', ctx: { finding: () => {} }, stop: (r) => { throw new NotRunError(r); } }), /no herdr-observed start/));
    check('#253 pastFloor: only a settled state at or past the floor ends the turn', pastFloor({ stateChangeSeq: 7 }, { floor: 6 }) && pastFloor({ stateChangeSeq: 6 }, { floor: 6 }) && !pastFloor({ stateChangeSeq: 5 }, { floor: 6 }) && !pastFloor({ stateChangeSeq: null }, { floor: 6 }) && pastFloor({ stateChangeSeq: null }, { floor: null }));
  }
  {
    // A push-started turn still running holds the next prompt back: the watch saw working@6;
    // herdr's settled answer idle@5 (from before the push) is not past it; idle@8 is. The
    // prompt (the operator question) is typed only after that.
    const h = stubHerdr({ gets: [{ state: 'idle', stateChangeSeq: 5 }, { state: 'idle', stateChangeSeq: 8 }], watch: { state: 'working', stateChangeSeq: 6 }, waits: [{ state: 'idle', stateChangeSeq: 5 }, { state: 'idle', stateChangeSeq: 5 }, { state: 'idle', stateChangeSeq: 8 }], prompted: { state: 'idle', stateChangeSeq: 10 } });
    const { agent, g } = agentWith(h);
    const w = await agent.watch('C2-push');
    const r = await agent.settledRead('after-C2', { lines: 50 }, { context: 'C2-turn', timeoutMs: 5000, since: w });
    await agent.prompt('the question', { wait: true });
    const firstPrompt = h.log.indexOf('prompt --wait');
    check('#253 settle(since watch): the push\'s turn still running (settled answers from before the observed working@6) holds the next prompt back until idle@8', r.settled?.stateChangeSeq === 8 && r.settled?.floor === 6 && /observed working/.test(r.settled?.turnBegunBy ?? '') && firstPrompt > h.log.lastIndexOf('full-read:recent-unwrapped') && g.herdrStates.filter((s) => s.context === 'C2-turn').length === 3, h.log.join());
    const h2 = stubHerdr({ gets: [{ state: 'working', stateChangeSeq: 9 }] });
    const a2 = agentWith(h2).agent;
    check('#253 prompt(wait): a prompt is never typed while herdr reports the agent working (NOT RUN, nothing sent)', (await rejectsWith(() => a2.prompt('q', { wait: true }), /never typed into a running turn/)) && !h2.log.some((x) => x.startsWith('prompt')));
    const h3 = stubHerdr({ gets: [{ state: 'working', stateChangeSeq: 9 }], waits: [{ state: 'idle', stateChangeSeq: 9 }, { state: 'idle', stateChangeSeq: 10 }] });
    const a3 = agentWith(h3).agent;
    const w3 = await a3.watch('C6-push');
    const r3 = await a3.settle('C6-turn', 5000, { since: w3 });
    check('#253 settle(since watch): a push into a running turn (baseline working@9) needs no watch; the turn ends past 9', w3.running && !h3.log.includes('watch') && r3.settled?.stateChangeSeq === 10, h3.log.join());
    const h4 = stubHerdr({ gets: [{ state: 'idle', stateChangeSeq: 5 }], prompted: { state: 'idle', stateChangeSeq: 7 }, waits: [{ state: 'idle', stateChangeSeq: 5 }, { state: 'idle', stateChangeSeq: 7 }] });
    const a4 = agentWith(h4).agent;
    const p4 = await a4.prompt('marker', { wait: true });
    const r4 = await a4.settle('marker-turn', 5000, { since: p4 });
    check('#253 prompt(wait) + settle(since): typed through `agent prompt --wait` (herdr observes the activity); the settled state must pass the queued state\'s seq', p4.kind === 'prompt-wait' && h4.log.slice(0, 2).join() === 'get,prompt --wait' && r4.settled?.stateChangeSeq === 7 && /prompt --wait/.test(r4.settled?.turnBegunBy ?? ''), h4.log.join());
  }

  // --- startup settle after a wire observation (#282) ------------------------------------------
  {
    const floorRec = await turnFloor({ since: { kind: 'floor', floor: 10, by: 'x' }, g: { herdrStates: [] }, context: 'x', ctx: { finding: () => {} }, stop: (r) => { throw new NotRunError(r); } });
    check('#282 turnFloor: a floor from an observation is taken as is (at or past it)', floorRec.floor === 10 && floorRec.by === 'x' && pastFloor({ stateChangeSeq: 10 }, floorRec) && !pastFloor({ stateChangeSeq: 9 }, floorRec));
  }
  {
    // Live run 20261004T075757Z: Codex's MCP connect seen, then `agent get` read working@10.
    // The startup settle waits for idle past it and re-checks; only then is the prompt typed.
    const h = stubHerdr({ gets: [{ state: 'working', stateChangeSeq: 10 }, { state: 'idle', stateChangeSeq: 11 }, { state: 'idle', stateChangeSeq: 11 }], waits: [{ state: 'idle', stateChangeSeq: 11 }], prompted: { state: 'idle', stateChangeSeq: 13 } });
    const { agent, g, findings } = agentWith(h);
    const s = await agent.startupSettle('codex-mcp-settle', "Codex's MCP connect", 5000);
    await agent.prompt('the tools prompt', { wait: true });
    check('#282 startupSettle: working@10 at the observation -> settled idle@11, re-checked idle@11, then (and only then) the prompt is typed', s.outcome === 'settled' && s.observed.state === 'working' && s.observed.stateChangeSeq === 10 && s.settles.length === 1 && s.settles[0].floor === 10 && s.settled.stateChangeSeq === 11 && h.log.join() === 'get,wait,read:visible,get,get,prompt --wait' && g.startupSettles?.[0] === s && findings.length === 0, h.log.join());
  }
  {
    // A stale settled answer below the floor is never taken (the floor bites).
    const h = stubHerdr({ gets: [{ state: 'working', stateChangeSeq: 10 }, { state: 'idle', stateChangeSeq: 11 }], waits: [{ state: 'idle', stateChangeSeq: 9 }, { state: 'idle', stateChangeSeq: 11 }] });
    const { agent } = agentWith(h);
    const s = await agent.startupSettle('codex-mcp-settle', 'x', 5000);
    check('#282 startupSettle: a settled answer below the observation\'s state_change_seq is not settled; the next one at or past it is', s.outcome === 'settled' && s.settles.length === 1 && s.settles[0].stateChangeSeq === 11 && h.log.filter((x) => x === 'wait').length === 2, h.log.join());
  }
  {
    // Idle at the observation, working again before the re-check: the re-check raises the floor.
    const h = stubHerdr({ gets: [{ state: 'idle', stateChangeSeq: 9 }, { state: 'working', stateChangeSeq: 10 }, { state: 'idle', stateChangeSeq: 11 }], waits: [{ state: 'idle', stateChangeSeq: 9 }, { state: 'idle', stateChangeSeq: 11 }] });
    const { agent } = agentWith(h);
    const s = await agent.startupSettle('codex-mcp-settle', 'x', 5000);
    check('#282 startupSettle: idle at the observation but working on the re-check is not settled; it settles again past the re-check and re-checks again', s.outcome === 'settled' && s.settles.length === 2 && s.rechecks.length === 2 && s.rechecks[0].state === 'working' && s.settles[1].floor === 10 && s.settled.stateChangeSeq === 11 && !h.log.some((x) => x.startsWith('prompt')), h.log.join());
  }
  {
    // #282 review (M4): idle@11 settled, idle@12 on the re-check (it went working and back
    // during settleMs) is NOT settled: the re-check needs the SAME state_change_seq.
    const h = stubHerdr({ gets: [{ state: 'working', stateChangeSeq: 10 }, { state: 'idle', stateChangeSeq: 12 }, { state: 'idle', stateChangeSeq: 12 }], waits: [{ state: 'idle', stateChangeSeq: 11 }, { state: 'idle', stateChangeSeq: 12 }] });
    const { agent } = agentWith(h);
    const s = await agent.startupSettle('codex-mcp-settle', 'x', 5000);
    check('#282 startupSettle: a re-check idle at a later state_change_seq (idle@11 settled, idle@12 re-checked) is not settled; it settles again past 12 and re-checks', s.outcome === 'settled' && s.settles.length === 2 && s.rechecks.length === 2 && s.rechecks[0].stateChangeSeq === 12 && s.settles[1].floor === 12 && s.settled.stateChangeSeq === 12 && h.log.join() === 'get,wait,read:visible,get,wait,read:visible,get', h.log.join());
  }
  {
    // #282 review: G2's plain operator prompt (no --wait) gets the #253 refusal too.
    for (const st of ['working', 'blocked']) {
      const h = stubHerdr({ gets: [{ state: st, stateChangeSeq: 9 }] });
      const findings = [];
      const g = { herdrStates: [] };
      const ok = await rejectsWith(() => refuseRunningTurn({ herdr: h, name: 'g2codex', g, context: 'operator-prompt', ctx: { finding: (f) => findings.push(f) }, stop: (r) => { throw new NotRunError(r); } }), /never typed into a running turn \(#253\)/);
      check(`#282/#253 refuseRunningTurn: herdr ${st} just before a plain prompt is NOT RUN; only the \`agent get\` was sent`, ok && h.log.join() === 'get' && g.herdrStates[0]?.context === 'operator-prompt:baseline (agent get)', h.log.join());
    }
    const h = stubHerdr({ gets: [{ state: 'idle', stateChangeSeq: 9 }] });
    const g = { herdrStates: [] };
    const base = await refuseRunningTurn({ herdr: h, name: 'g2codex', g, context: 'operator-prompt', ctx: { finding: () => {} }, stop: (r) => { throw new NotRunError(r); } });
    const hn = stubHerdr({ gets: [{ state: null, stateChangeSeq: null }] });
    const nullFails = await rejectsWith(() => refuseRunningTurn({ herdr: hn, name: 'g2codex', g: { herdrStates: [] }, context: 'operator-prompt', ctx: { finding: () => {} }, stop: (r) => { throw new NotRunError(r); } }), /did not report the agent's state/);
    check('#282/#253 refuseRunningTurn: idle returns the recorded baseline; a baseline naming no state fails closed', base.state === 'idle' && base.stateChangeSeq === 9 && nullFails);
  }
  {
    // Stays working: herdr's own wait times out inside the settle -> NOT RUN naming the startup settle.
    const h = stubHerdr({ gets: [{ state: 'working', stateChangeSeq: 10 }], waits: [new NotRunError('herdr agent wait (wait) timed out (herdr timeout); run ends NOT RUN, nothing re-submitted')] });
    const { agent, g, findings } = agentWith(h);
    const ok = await rejectsWith(() => agent.startupSettle('codex-mcp-settle', "Codex's MCP connect", 5000), /startup settle after Codex's MCP connect did not complete/);
    check('#282 startupSettle: an agent that never leaves working ends NOT RUN with one finding naming the startup settle; nothing typed', ok && findings.filter((f) => /^startup settle \(#282\)/.test(f)).length === 1 && /did not settle after Codex's MCP connect/.test(findings[0]) && g.startupSettles[0].outcome.startsWith('not settled') && !h.log.some((x) => x.startsWith('prompt')), JSON.stringify([h.log, findings]));
    const h2 = stubHerdr({ gets: [{ state: 'working', stateChangeSeq: 10 }], thenWait: { state: 'working', stateChangeSeq: 10 } });
    const a2 = agentWith(h2);
    const ok2 = await rejectsWith(() => a2.agent.startupSettle('codex-mcp-settle', 'x', 40), /startup settle after x did not complete/);
    check('#282 startupSettle: bounded; no idle within the bound is NOT RUN with the startup-settle finding (never typed)', ok2 && a2.findings.filter((f) => /^startup settle \(#282\)/.test(f)).length === 1 && !h2.log.some((x) => x.startsWith('prompt')), JSON.stringify(a2.findings));
    const h3 = stubHerdr({ gets: [{ state: 'idle', stateChangeSeq: null }] });
    const a3 = agentWith(h3);
    check('#282 startupSettle: an observation with no state_change_seq cannot be ordered on: NOT RUN, finding', (await rejectsWith(() => a3.agent.startupSettle('s', 'x', 5000), /no state_change_seq/)) && a3.findings.some((f) => /^startup settle \(#282\)/.test(f)));
    const h4 = stubHerdr({ gets: [{ state: null, stateChangeSeq: null }] });
    const a4 = agentWith(h4);
    check('#282 startupSettle: an observation `agent get` naming no state fails closed (#253), with the startup-settle finding too', (await rejectsWith(() => a4.agent.startupSettle('s', 'x', 5000), /startup settle/)) && a4.findings.some((f) => /#253/.test(f)) && a4.findings.some((f) => /^startup settle \(#282\)/.test(f)));
  }

  // --- settled reads (#246) ------------------------------------------------------------------
  {
    const h = stubHerdr({ reads: [{ errorCode: 'agent_not_idle' }, { errorCode: null }] });
    const { agent, g } = agentWith(h);
    const r = await agent.settledRead('after-X3', { source: 'recent-unwrapped', lines: 50 }, { context: 'X3-turn', timeoutMs: 5000, done: async () => 'turn X completed' });
    check('#246 settledRead: the after-delivery read waits for idle first; after agent_not_idle it settles again and reads once more; nothing is sent', h.log.join() === 'wait,read:visible,full-read:recent-unwrapped:agent_not_idle,wait,read:visible,full-read:recent-unwrapped' && r.text === 'full history' && g.notIdleRefusals?.length === 1 && !h.log.some((x) => x.startsWith('prompt')), h.log.join());
  }
  {
    const h = stubHerdr({ reads: [{ errorCode: 'agent_not_idle' }, { errorCode: 'agent_not_idle' }, { errorCode: 'agent_not_idle' }, { errorCode: null }] });
    const { agent } = agentWith(h);
    check('#246 settledRead: three refusals after settling end the run NOT RUN (bounded)', (await rejectsWith(() => agent.settledRead('after-X3', { lines: 50 }, { timeoutMs: 5000 }), /refused the recent-unwrapped read 3 times/)) && h.log.filter((x) => x.startsWith('full-read')).length === 3);
  }
  {
    const h = stubHerdr({ waits: [{ state: 'unknown', stateChangeSeq: 1 }], reads: [{ errorCode: 'agent_not_idle' }] });
    const { agent, findings } = agentWith(h);
    const r = await agent.settledRead('after-B4', { lines: 50 }, { timeoutMs: 5000, done: async () => 'the X4 queued turn Q completed on the wire (thread/turns/list)' });
    check('#246 settledRead: herdr `unknown` while the wire shows the turn over -> the visible screen instead, with a finding naming that wire evidence', r.fellBackToVisible === true && h.log.at(-1) === 'read:visible' && findings.some((f) => /fell back to the visible screen/.test(f) && /the X4 queued turn Q completed on the wire/.test(f)));
  }

  // --- the wire-level "turn finished" signal ---------------------------------------------------
  {
    const tl = (line, turns, threadId = 'T') => ({ line, threadId, error: null, turns });
    const t = (id, status, text) => ({ id, status, userTexts: text ? [text] : [] });
    const facts = (lists) => ({ turnsLists: lists });
    check('#253 threadIdleOnWire: a turn inProgress is not idle; all completed is; the marker must be in a completed turn; another thread\'s list or an older list does not count',
      threadIdleOnWire(facts([tl(5, [t('b', 'inProgress', 'frame'), t('a', 'completed', 'Hi marker')])]), 'T') === null &&
      threadIdleOnWire(facts([tl(5, [t('a', 'inProgress', 'Hi marker')])]), 'T', { marker: 'Hi marker' }) === null &&
      threadIdleOnWire(facts([tl(5, [t('a', 'completed', 'Hi marker')])]), 'T', { marker: 'Hi marker' })?.markerTurnId === 'a' &&
      threadIdleOnWire(facts([tl(5, [t('a', 'completed', 'other')])]), 'T', { marker: 'Hi marker' }) === null &&
      threadIdleOnWire(facts([tl(5, [t('a', 'completed', 'x')], 'U')]), 'T') === null &&
      threadIdleOnWire(facts([tl(5, [t('a', 'completed', 'x')])]), 'T', { sinceLine: 5 }) === null &&
      threadIdleOnWire(facts([tl(5, [t('a', 'completed', 'x')]), tl(9, [t('b', 'inProgress', 'y'), t('a', 'completed', 'x')])]), 'T') === null);
  }

  // --- fake-herdr imitates herdr's answer shape and the agent_not_idle refusal ----------------
  {
    const f = fakeSession('');
    try {
      const w = await f.herdr.agentWait('a', { until: ['idle', 'done', 'blocked', 'unknown'], timeoutMs: 3000 });
      check('#253 fake-herdr: agent wait answers with agent_status and state_change_seq (no `state` field), and the driver reads them', w.state === 'idle' && w.stateChangeSeq === 3 && w.json?.result?.agent?.state === undefined);
      f.setAgent('working', 4);
      const refused = await f.herdr.agentReadResult('a', { source: 'recent-unwrapped', lines: 200, allowErrorCodes: ['agent_not_idle'] });
      const visible = await f.herdr.agentReadResult('a', { source: 'visible' });
      check('#246 fake-herdr: an `agent read --lines` of history while the agent is working is refused agent_not_idle; a visible read is not', refused.errorCode === 'agent_not_idle' && refused.text === null && visible.errorCode === null && /visible screen/.test(visible.text));
      check('#246 fake-herdr: without allowErrorCodes the refusal fails the read (exit 1, agent_not_idle)', await f.herdr.agentReadResult('a', { source: 'recent', lines: 200 }).then(() => false, (e) => /agent_not_idle/.test(e.message)));
    } finally {
      f.cleanup();
    }
  }
  {
    const f = fakeSession('linger-working=60000', { state: 'idle', seq: 6, at: Date.now() });
    try {
      const r = await f.herdr.agentReadResult('a', { source: 'recent-unwrapped', lines: 200, allowErrorCodes: ['agent_not_idle'] });
      const g = await f.herdr.agentGet('a');
      check('#246 fake-herdr linger-working: just after the harness double went idle (the wire turn done), herdr still says working and refuses the read', r.errorCode === 'agent_not_idle' && g?.agent_status === 'working' && g?.state_change_seq === 5);
    } finally {
      f.cleanup();
    }
  }
  {
    const f = fakeSession('');
    try {
      // Each state change lands only after the fake has taken its baseline (waits.log), never on a
      // timer: whatever the polls happen to see, the answer is the same.
      const pending = f.herdr.agentPrompt('a', 'q', { wait: { until: ['idle', 'done', 'blocked', 'unknown'], timeoutMs: 8000 } });
      const armed0 = await f.waitArmed(1);
      f.setAgent('unknown', 4); // a flicker past the baseline: not activity
      f.setAgent('working', 5);
      f.setAgent('idle', 6);
      const p = await pending;
      check('#253 fake-herdr prompt --wait: an unknown flicker does not count; it returns the settled state after an observed working past the queued seq', armed0 && p.state === 'idle' && p.stateChangeSeq === 6, JSON.stringify([armed0, p.state, p.stateChangeSeq]));
      // A working state too short for any poll: herdr's event stream still carries it. The
      // transition is logged only (no snapshot ever shows it), and only after the wait has
      // taken its event position, so the match can come from the event stream alone; herdr then
      // reports it with the agent's state_change_seq at the match (here the idle@8 after it).
      const watch = f.herdr.agentWait('a', { until: ['working', 'blocked'], timeoutMs: 8000, background: true });
      const armed = await f.waitArmed(2);
      f.logOnly('working', 7);
      f.setAgent('idle', 8);
      const wr = await watch;
      check('#253 fake-herdr agent wait: a transient working after the request (never a current state) is matched, as herdr\'s event-driven wait does, reported with the then-current state_change_seq', armed && wr.state === 'working' && wr.stateChangeSeq === 8, JSON.stringify([armed, wr.state, wr.stateChangeSeq]));
      // A working state that may also be seen current: matched either way, its seq at least the transition's.
      const watch2 = f.herdr.agentWait('a', { until: ['working', 'blocked'], timeoutMs: 8000, background: true });
      const armed2 = await f.waitArmed(3);
      f.setAgent('working', 9);
      f.setAgent('idle', 10);
      const wr2 = await watch2;
      check('#253 fake-herdr agent wait: a working state after the request is matched whether a poll saw it current or only in the event stream, with state_change_seq >= the transition\'s', armed2 && wr2.state === 'working' && wr2.stateChangeSeq >= 9 && wr2.stateChangeSeq <= 10, JSON.stringify([armed2, wr2.state, wr2.stateChangeSeq]));
      f.setAgent('idle', 11);
      check('#253 fake-herdr prompt --wait: no working or blocked within 5 s is agent_prompt_stalled (the run ends NOT RUN)', await rejectsWith(() => f.herdr.agentPrompt('a', 'q2', { wait: { until: ['idle'], timeoutMs: 8000 } }), /agent_prompt_stalled/));
    } finally {
      f.cleanup();
    }
  }
  {
    const f = fakeSession('wait-no-status');
    try {
      const w = await f.herdr.agentWait('a', { until: ['idle'], timeoutMs: 3000 });
      check('#253 fake-herdr wait-no-status: a wait answer with no agent_status reads as state null', w.exitCode === 0 && w.state === null && w.stateChangeSeq === null);
    } finally {
      f.cleanup();
    }
  }
}
