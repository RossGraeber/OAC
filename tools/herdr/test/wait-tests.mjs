// Unit checks for herdr waits, settles and settled reads (#253, #246). In-process and
// cross-platform: the fake-herdr checks spawn test/fake-herdr.mjs with node only (no pane
// shell), so they run on Windows too.
//
// What they pin:
//   - herdr v0.9.1 reports an agent's state as AgentInfo.agent_status; a response without one
//     (or the `state` field the driver used to read) is not a state: null, never settled;
//   - a settle whose wait returns null ends the run NOT RUN with a finding;
//   - a settle after a prompt needs that prompt's turn to have begun (state_change_seq past the
//     prompt's), and `unknown` is not settled without the wire;
//   - an after-delivery read waits for idle first, retries after agent_not_idle, and gives up
//     NOT RUN after three refusals; nothing is re-sent;
//   - fake-herdr imitates herdr's agent_not_idle refusal and the agent_status shape.

import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { HerdrSession, NotRunError, agentStatusOf, stateChangeSeqOf } from '../lib/herdr.mjs';
import { makeAgent, recordWaitState, promptTurnBegun } from '../lib/gate-common.mjs';
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

// A scripted stand-in for HerdrSession: each agentWait / agentReadResult answer comes from a
// queue; every call is logged in order.
function stubHerdr({ waits = [], reads = [], promptSeq = 5, thenWait = null } = {}) {
  const log = [];
  const commands = [];
  const entry = (kind) => {
    const e = { seq: commands.length + 1, kind, startedAt: new Date().toISOString(), endedAt: new Date().toISOString(), durationMs: 1 };
    commands.push(e);
    log.push(kind);
    return e;
  };
  const lastWait = thenWait ?? { state: 'idle', stateChangeSeq: promptSeq };
  return {
    log,
    commands,
    abortSignal: null,
    async agentWait() {
      const w = waits.length ? waits.shift() : lastWait;
      return { entry: entry('wait'), json: null, state: w.state, stateChangeSeq: w.stateChangeSeq ?? null };
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
    async agentPrompt() {
      return { entry: entry('prompt'), stateChangeSeq: promptSeq };
    },
  };
}

function agentWith(herdr, { findings = [] } = {}) {
  const g = { herdrStates: [], dialogs: [] };
  const ctx = { herdr, remainingMs: () => 60000, finding: (f) => findings.push(f), sessionName: 'unit' };
  const stop = (reason) => {
    throw new NotRunError(reason);
  };
  const params = { settleMs: 0, pollMs: 1, maxDialogs: 3, humanAcceptTimeoutMs: 1000 };
  const agent = makeAgent({ ctx, g, name: 'unit', label: 'codex', classify: (t) => ({ busy: /BUSY/.test(t), dialog: null }), dialogKinds: {}, driverMayAccept: () => null, accept: 'driver', num: (k) => params[k], stop });
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
    writeFileSync(join(adir, 'state'), s);
    writeFileSync(join(adir, 'state-seq'), `${n} ${t}`);
  };
  setAgent(state, seq, at);
  const herdr = new HerdrSession({ herdrCmd: [process.execPath, FAKE], sessionName: name, env: { ...process.env, FAKE_HERDR_STATE: root, FAKE_HERDR_MODE: mode }, cwd: root, timebox: { remainingMs: () => 60000 }, commands: [], defaultDeadlineMs: 15000 });
  return { herdr, setAgent, cleanup: () => rmSync(root, { recursive: true, force: true }) };
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
  {
    const h = stubHerdr({ thenWait: { state: 'unknown', stateChangeSeq: 1 } });
    const { agent } = agentWith(h);
    check('#253 settle: `unknown` is not settled without a wire-level signal (times out NOT RUN)', await rejectsWith(() => agent.settle('turn', 60), /did not settle/));
    const h2 = stubHerdr({ waits: [{ state: 'unknown', stateChangeSeq: 1 }] });
    const r = await agentWith(h2).agent.settle('turn', 5000, { done: () => true });
    check('#253 settle: `unknown` with the wire showing the turn over settles, recorded as such', r.settled?.state === 'unknown' && r.settled?.by === 'state');
  }

  // --- a settle after a prompt waits for that prompt's turn --------------------------------
  {
    const h = stubHerdr({ waits: [{ state: 'idle', stateChangeSeq: 5 }, { state: 'idle', stateChangeSeq: 5 }, { state: 'idle', stateChangeSeq: 7 }], promptSeq: 5 });
    const { agent, g } = agentWith(h);
    const p = await agent.prompt('hello');
    const r = await agent.settle('marker-turn', 5000, { since: p });
    check('#253 settle(since): an idle answer from before the prompt picked up (state_change_seq not past the prompt\'s) is not settled; the turn\'s own idle is', p.stateChangeSeq === 5 && g.herdrStates.length === 3 && r.settled?.by === 'state_change_seq' && r.settled?.stateChangeSeq === 7, JSON.stringify(g.herdrStates));
    const h2 = stubHerdr({ waits: [{ state: 'idle', stateChangeSeq: 5 }], promptSeq: 5 });
    const a2 = agentWith(h2).agent;
    const p2 = await a2.prompt('x');
    check('#253 settle(since): a turn herdr never shows beginning is not settled (times out NOT RUN, nothing sent)', (await rejectsWith(() => a2.settle('marker-turn', 60, { since: p2 }), /did not settle/)) && !h2.log.slice(1).includes('prompt'));
    const h3 = stubHerdr({ waits: [{ state: 'idle', stateChangeSeq: 5 }], promptSeq: 5 });
    const a3 = agentWith(h3).agent;
    const r3 = await a3.settle('q', 5000, { since: await a3.prompt('x'), done: () => true });
    check('#253 settle(since): the wire showing the turn done stands in for state_change_seq', r3.settled?.by === 'wire');
    check('#253 promptTurnBegun: no since -> begun; seq past the baseline, a busy screen or the wire -> begun; otherwise not', promptTurnBegun({ since: null }) && promptTurnBegun({ since: { stateChangeSeq: 2 }, stateChangeSeq: 3 }) && !promptTurnBegun({ since: { stateChangeSeq: 2 }, stateChangeSeq: 2 }) && !promptTurnBegun({ since: { stateChangeSeq: 2 }, stateChangeSeq: null }) && promptTurnBegun({ since: { stateChangeSeq: 2 }, stateChangeSeq: 2, busySeen: true }) && promptTurnBegun({ since: { stateChangeSeq: null }, wireDone: true }));
  }

  // --- settled reads (#246) ------------------------------------------------------------------
  {
    const h = stubHerdr({ reads: [{ errorCode: 'agent_not_idle' }, { errorCode: null }] });
    const { agent, g } = agentWith(h);
    const r = await agent.settledRead('after-X3', { source: 'recent-unwrapped', lines: 50 }, { context: 'X3-turn', timeoutMs: 5000, done: () => true });
    check('#246 settledRead: the after-delivery read waits for idle first; after agent_not_idle it settles again and reads once more; nothing is sent', h.log.join() === 'wait,read:visible,full-read:recent-unwrapped:agent_not_idle,wait,read:visible,full-read:recent-unwrapped' && r.text === 'full history' && g.notIdleRefusals?.length === 1 && !h.log.includes('prompt'), h.log.join());
  }
  {
    const h = stubHerdr({ reads: [{ errorCode: 'agent_not_idle' }, { errorCode: 'agent_not_idle' }, { errorCode: 'agent_not_idle' }, { errorCode: null }] });
    const { agent } = agentWith(h);
    check('#246 settledRead: three refusals after settling end the run NOT RUN (bounded)', (await rejectsWith(() => agent.settledRead('after-X3', { lines: 50 }, { timeoutMs: 5000 }), /refused the recent-unwrapped read 3 times/)) && h.log.filter((x) => x.startsWith('full-read')).length === 3);
  }
  {
    const h = stubHerdr({ waits: [{ state: 'unknown', stateChangeSeq: 1 }], reads: [{ errorCode: 'agent_not_idle' }] });
    const { agent, findings } = agentWith(h);
    const r = await agent.settledRead('after-B4', { lines: 50 }, { timeoutMs: 5000, done: () => true });
    check('#246 settledRead: herdr `unknown` while the wire shows the turn over -> the visible screen instead, with a finding', r.fellBackToVisible === true && h.log.at(-1) === 'read:visible' && findings.some((f) => /fell back to the visible screen/.test(f)));
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
    const f = fakeSession('wait-no-status');
    try {
      const w = await f.herdr.agentWait('a', { until: ['idle'], timeoutMs: 3000 });
      check('#253 fake-herdr wait-no-status: a wait answer with no agent_status reads as state null', w.exitCode === 0 && w.state === null && w.stateChangeSeq === null);
    } finally {
      f.cleanup();
    }
  }
}
