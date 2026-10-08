#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
//
// Self-test for the fake Codex app-server (#58, F9). Node built-ins only; no Codex, no
// credential, no network beyond loopback.
//
//   node tests/fakes/codex-app-server/self-test.mjs
//
// Three parts:
//   1. Replay: drive the fake through the D6 attempt-2 sequence (conn1 thread/start, conn3
//      turn/start x2 and thread/queue/add, conn2 thread/resume) and compare every frame
//      each connection receives with the recorded D6 transcript: same order, same kind
//      (response / notification method), same JSON shape (members and value types).
//   2. Behaviours: the experimental-API gate, queued-until-idle, the queue refusals, the
//      idle add, the unloaded add and the interrupted turn against the S3 capture (#343), the
//      interrupt wait, steering detection, override flags, list methods (README table).
//   3. Transports: the real server.mjs process over stdio and a loopback WebSocket.
// Exit 0 when every case passes, 1 otherwise.

import { spawn } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { createInterface } from 'node:readline';
import { FIXTURE_FILES, REPO_ROOT, fixturePath, readTranscript } from './lib/fixtures.mjs';
import { FakeCodexAppServer, NOT_MODELLED, PRIOR_TURNS_PREVIEW } from './lib/model.mjs';
import { connectWs } from './lib/ws.mjs';

const SERVER = join(REPO_ROOT, 'tests', 'fakes', 'codex-app-server', 'server.mjs');
let failed = 0;
let passed = 0;
const results = [];
async function test(name, fn) {
  try {
    await fn();
    passed++;
    results.push(`pass  ${name}`);
  } catch (e) {
    failed++;
    results.push(`FAIL  ${name}\n        ${String(e?.stack ?? e).split('\n').slice(0, 4).join('\n        ')}`);
  }
}
function assert(cond, msg) {
  if (!cond) throw new Error(msg);
}
const eq = (a, b, msg) => assert(JSON.stringify(a) === JSON.stringify(b), `${msg}: got ${JSON.stringify(a)}, want ${JSON.stringify(b)}`);

// An in-process client: every frame the fake sends lands in `frames`.
function client(fake, name, { experimental = true, init = true } = {}) {
  const frames = [];
  const conn = fake.connect((o) => frames.push(structuredClone(o)));
  let nextId = 0;
  const c = {
    frames,
    conn,
    request(method, params = {}) {
      const id = nextId++;
      conn.receive({ jsonrpc: '2.0', id, method, params });
      const r = frames.find((f) => f.id === id && f.method === undefined);
      assert(r, `no response to ${method}`);
      return r;
    },
    ok(method, params) {
      const r = c.request(method, params);
      assert(r.result !== undefined, `${method} failed: ${JSON.stringify(r.error)}`);
      return r.result;
    },
    notifications: (method) => frames.filter((f) => f.method === method),
  };
  if (init) {
    c.request('initialize', { clientInfo: { name, title: name, version: '0.0.1' }, capabilities: experimental ? { experimentalApi: true } : {} });
    conn.receive({ jsonrpc: '2.0', method: 'initialized', params: {} });
  }
  return c;
}
const control = (fake, method, params = {}) => {
  const ctl = client(fake, 'oac-fake-control', { init: false });
  const r = ctl.request(method, params);
  assert(r.result !== undefined, `${method} failed: ${JSON.stringify(r.error)}`);
  return r.result;
};

// ---- 1. replay ------------------------------------------------------------------------

// Frames the fake does not emit, by design (README "Omitted notifications").
const OMITTED = new Set(['mcpServer/startupStatus/updated', 'thread/tokenUsage/updated', 'account/rateLimits/updated']);

function recordedReceived(key) {
  const out = [];
  for (const f of readTranscript(key)) {
    if (f.direction !== 'daemon->client') continue;
    const m = f.payload.method;
    if (OMITTED.has(m)) continue;
    // The recorded run streamed several deltas per message; the fake sends one.
    if (m === 'item/agentMessage/delta' && out.at(-1)?.payload.method === m) continue;
    out.push(f);
  }
  return out;
}

// JSON shape: members and value types, recursively; arrays by length and element shape.
function shape(v) {
  if (v === null) return 'null';
  if (Array.isArray(v)) return v.map(shape);
  if (typeof v === 'object') return Object.fromEntries(Object.keys(v).sort().map((k) => [k, shape(v[k])]));
  return typeof v;
}
const kind = (p) => p.method ?? (p.error ? 'error' : 'result');

function compareConnection(label, key, got, { dropQueueChanged = false } = {}) {
  let want = recordedReceived(key).map((f) => ({ line: f.line, payload: f.payload }));
  let have = got;
  if (dropQueueChanged) {
    // Recorded thread/queue/changed timing interleaves with model-paced item events the
    // fake emits at once, so its position is checked by the caller. Its shape is checked
    // here, against every recorded thread/queue/changed frame.
    const recordedQc = want.filter((f) => f.payload.method === 'thread/queue/changed');
    const fakeQc = have.filter((p) => p.method === 'thread/queue/changed');
    assert(recordedQc.length > 0 && fakeQc.length > 0, `${label}: thread/queue/changed present`);
    for (const p of fakeQc) {
      for (const w of recordedQc) {
        assert(
          JSON.stringify(shape(p)) === JSON.stringify(shape(w.payload)),
          `${label}: thread/queue/changed shape differs from ${fixturePath(key)}:${w.line}`,
        );
      }
    }
    want = want.filter((f) => f.payload.method !== 'thread/queue/changed');
    have = have.filter((p) => p.method !== 'thread/queue/changed');
  }
  eq(have.map(kind), want.map((f) => kind(f.payload)), `${label}: frame sequence`);
  have.forEach((p, i) => {
    const w = want[i];
    const a = JSON.stringify(shape(p));
    const b = JSON.stringify(shape(w.payload));
    assert(a === b, `${label}: frame ${i} (${kind(p)}) shape differs from ${fixturePath(key)}:${w.line}\n  got  ${a.slice(0, 400)}\n  want ${b.slice(0, 400)}`);
  });
}

function textOf(key, method) {
  const f = readTranscript(key).find((x) => x.direction === 'client->daemon' && x.payload.method === method);
  return f.payload.params;
}
function agentTexts() {
  return readTranscript('d6Conn1')
    .filter((f) => f.payload.method === 'item/completed' && f.payload.params.item.type === 'agentMessage')
    .map((f) => f.payload.params.item.text);
}

await test('replay: D6 attempt-2 sequence reproduces every recorded frame kind, order and shape (conn1, conn2, conn3 x3)', () => {
  const fake = new FakeCodexAppServer();
  const conn1 = client(fake, 'oac_d6_spike');
  const start = conn1.ok('thread/start', textOf('d6Conn1', 'thread/start'));
  const threadId = start.thread.id;
  const [ack, list, queued] = agentTexts();

  const t1 = client(fake, 'oac_d6_spike');
  t1.ok('turn/start', { ...textOf('d6Turn1', 'turn/start'), threadId });
  control(fake, 'oacFake/turn/complete', { threadId, agentText: ack });

  const conn2 = client(fake, 'oac_d6_spike');
  conn2.ok('thread/resume', { ...textOf('d6Conn2', 'thread/resume'), threadId });

  const t2 = client(fake, 'oac_d6_spike');
  t2.ok('turn/start', { ...textOf('d6Turn2', 'turn/start'), threadId });
  const q = client(fake, 'oac_d6_spike');
  q.ok('thread/queue/add', { ...textOf('d6Queue', 'thread/queue/add'), threadId });
  control(fake, 'oacFake/turn/complete', { threadId, agentText: list });
  control(fake, 'oacFake/turn/complete', { threadId, agentText: queued });

  compareConnection('conn1', 'd6Conn1', conn1.frames, { dropQueueChanged: true });
  compareConnection('conn2', 'd6Conn2', conn2.frames, { dropQueueChanged: true });
  compareConnection('conn3 turn1', 'd6Turn1', t1.frames);
  compareConnection('conn3 turn2', 'd6Turn2', t2.frames);
  compareConnection('conn3 queue', 'd6Queue', q.frames);
  // thread/queue/changed: as recorded, once at the add and once at dispatch, and the
  // dispatch one directly precedes the dispatched turn's status change.
  for (const [label, c, key] of [['conn1', conn1, 'd6Conn1'], ['conn2', conn2, 'd6Conn2']]) {
    const recorded = recordedReceived(key).filter((f) => f.payload.method === 'thread/queue/changed').length;
    eq(c.notifications('thread/queue/changed').length, recorded, `${label}: thread/queue/changed count`);
    const i = c.frames.findLastIndex((f) => f.method === 'thread/queue/changed');
    eq([c.frames[i + 1].method, c.frames[i + 2].method], ['thread/status/changed', 'turn/started'], `${label}: dispatch order`);
  }
  // The queued input runs in a new turn, under its clientUserMessageId (D6 conn1).
  const qid = textOf('d6Queue', 'thread/queue/add').clientUserMessageId;
  const userItems = conn1.notifications('item/started').filter((f) => f.params.item.type === 'userMessage');
  eq(userItems.map((f) => f.params.item.clientId), [null, null, qid], 'userMessage clientIds');
  const turnIds = conn1.notifications('turn/started').map((f) => f.params.turn.id);
  eq(new Set(turnIds).size, 3, 'three distinct turns');
  eq(userItems[2].params.turnId, turnIds[2], 'queued input runs in the third turn');
});

await test('replay check catches a planted difference (missing member, extra frame, wrong type)', () => {
  const fake = new FakeCodexAppServer();
  const c = client(fake, 'oac_d6_spike');
  c.ok('turn/start', { threadId: client(fake, 'tui').ok('thread/start', {}).thread.id, input: [{ type: 'text', text: 'x' }] });
  compareConnection('control', 'd6Turn1', c.frames);
  const plants = [
    (f) => delete f.at(-1).result.turn.itemsView,
    (f) => f.push({ method: 'thread/queue/changed', params: {} }),
    (f) => (f.at(-1).result.turn.startedAt = 0),
  ];
  for (const plant of plants) {
    const frames = structuredClone(c.frames);
    plant(frames);
    let threw = false;
    try {
      compareConnection('planted', 'd6Turn1', frames);
    } catch {
      threw = true;
    }
    assert(threw, `planted difference not caught: ${plant}`);
  }
});

await test('replay: thread/resume before the first turn fails as recorded (D6 attempt 1)', () => {
  const fake = new FakeCodexAppServer();
  const c = client(fake, 'oac_d6_spike');
  const id = c.ok('thread/start', {}).thread.id;
  const r = c.request('thread/resume', { threadId: id, excludeTurns: true });
  const recorded = readTranscript('d6ResumeFail').find((f) => f.payload.error).payload.error;
  const recordedId = textOf('d6ResumeFail', 'thread/resume').threadId;
  eq(r.error, { code: recorded.code, message: recorded.message.replace(recordedId, id) }, 'error');
});

// ---- 2. behaviours --------------------------------------------------------------------

function idleThread(fake) {
  const tui = client(fake, 'tui');
  const threadId = tui.ok('thread/start', {}).thread.id;
  tui.ok('turn/start', { threadId, input: [{ type: 'text', text: 'first' }] });
  control(fake, 'oacFake/turn/complete', { threadId, agentText: 'ok' });
  return { tui, threadId };
}
const add = (c, threadId, text = 'hello', extra = {}) =>
  c.request('thread/queue/add', { threadId, clientUserMessageId: `cid-${text}`, input: [{ type: 'text', text }], ...extra });

await test('experimental gate: thread/queue/add without capabilities.experimentalApi is refused (-32600)', () => {
  const fake = new FakeCodexAppServer();
  const { threadId } = idleThread(fake);
  const c = client(fake, 'adapter', { experimental: false });
  const r = add(c, threadId);
  eq(r.error, { code: -32600, message: 'thread/queue/add requires experimentalApi capability' }, 'error');
  const log = control(fake, 'oacFake/calls', { clientName: 'adapter' });
  assert(log.calls.at(-1).flags.experimentalGateRefused === true, 'gate refusal flagged');
  eq(control(fake, 'oacFake/thread/state', { threadId }).queue, [], 'nothing queued');
});

await test('initialize gate: a request before initialize is refused "Not initialized"', () => {
  const fake = new FakeCodexAppServer();
  const c = client(fake, 'x', { init: false });
  eq(c.request('thread/loaded/list', {}).error, { code: -32600, message: 'Not initialized' }, 'error');
});

await test('queue: an add during a running turn waits for turn/completed and runs in a new turn', () => {
  const fake = new FakeCodexAppServer();
  const { tui, threadId } = idleThread(fake);
  tui.ok('turn/start', { threadId, input: [{ type: 'text', text: 'busy' }] });
  const running = control(fake, 'oacFake/thread/state', { threadId }).activeTurnId;
  const adapter = client(fake, 'adapter');
  const r = add(adapter, threadId, 'oac');
  assert(r.result.queuedSubmission.clientUserMessageId === 'cid-oac', 'queuedSubmission echoed');
  eq(control(fake, 'oacFake/thread/state', { threadId }).activeTurnId, running, 'still the running turn');
  eq(control(fake, 'oacFake/thread/state', { threadId }).queue.length, 1, 'queued');
  const done = control(fake, 'oacFake/turn/complete', { threadId });
  assert(done.dispatched && done.dispatched !== running, 'a new turn was dispatched');
  const st = control(fake, 'oacFake/thread/state', { threadId });
  eq(st.turns.at(-1).items.some((i) => i.clientId === 'cid-oac'), false, 'never joined the running turn');
  eq(st.queue, [], 'queue drained');
  const started = tui.notifications('item/started').at(-1).params;
  eq([started.turnId, started.item.clientId], [done.dispatched, 'cid-oac'], 'input arrives under the new turn');
});

// The frames one subscribed connection receives from a request on to the queued input's
// userMessage, as kinds: the recorded run's notifications the fake omits by design, and
// model-paced items (reasoning, agent text, hooks), are left out.
const QUEUE_KINDS = new Set(['thread/status/changed', 'thread/goal/cleared', 'thread/queue/changed', 'turn/started', 'item/started', 'item/completed']);
const queueKinds = (frames) =>
  frames
    .filter((p) => p.method === undefined || (QUEUE_KINDS.has(p.method) && (!p.params?.item || p.params.item.type === 'userMessage')))
    .map((p) => (p.method === undefined ? kind(p) : `${p.method}${p.params?.status?.type ? ` ${p.params.status.type}` : ''}`));
// From the S3 capture: the frames `mode` received from its request carrying `words` (or
// `method`) up to the userMessage item/completed of the queued input `clientId` names.
function recordedQueueKinds(mode, pick) {
  const fr = readTranscript('s3');
  const req = fr.find((f) => f.direction === 'client->daemon' && f.mode === mode && pick(f.payload));
  assert(req, `S3 capture: no ${mode} request`);
  const out = [];
  for (const f of fr.filter((x) => x.line > req.line && x.mode === mode && x.direction === 'daemon->client')) {
    out.push(f.payload);
    if (f.payload.method === 'item/completed' && f.payload.params?.item?.type === 'userMessage') break;
  }
  return { req, kinds: queueKinds(out) };
}

await test('queue: an add to an idle, loaded thread starts a turn at once, frames in the recorded order (S3 capture)', () => {
  const fake = new FakeCodexAppServer();
  const { tui, threadId } = idleThread(fake);
  const before = tui.frames.length;
  const r = add(tui, threadId, 'idle'); // the subscribed connection adds, as the S3 `cases-main` did
  assert(r.result, 'accepted');
  const st = control(fake, 'oacFake/thread/state', { threadId });
  assert(st.activeTurnId && st.queue.length === 0, 'the add started a turn and left nothing queued');
  const rec = recordedQueueKinds('cases-main', (p) => p.method === 'thread/queue/add' && JSON.stringify(p.params.input).includes('OAC S3 EXTRA MEMBER'));
  eq(queueKinds(tui.frames.slice(before)), rec.kinds, `frame order against ${fixturePath('s3')}:${rec.req.line}`);
  const item = tui.notifications('item/started').at(-1).params;
  eq([item.turnId, item.item.clientId], [st.activeTurnId, 'cid-idle'], 'the turn carries clientId = clientUserMessageId');
  const adapter = client(fake, 'adapter');
  control(fake, 'oacFake/turn/complete', { threadId });
  add(adapter, threadId);
  eq(adapter.frames.filter((f) => f.method === 'turn/started').length, 0, 'an adding connection that is not subscribed gets its response only');
});

await test('queue: an add to an unloaded thread waits, and dispatches once the thread is loaded (S3 capture)', () => {
  const fake = new FakeCodexAppServer();
  const threadId = control(fake, 'oacFake/thread/create', { loaded: false }).threadId;
  const adapter = client(fake, 'adapter');
  assert(add(adapter, threadId, 'unloaded').result, 'accepted');
  let st = control(fake, 'oacFake/thread/state', { threadId });
  eq([st.loaded, st.activeTurnId, st.queue.length], [false, null, 1], 'queued, not loaded, no turn');
  const reload = client(fake, 'reload');
  const before = reload.frames.length;
  reload.ok('thread/resume', { threadId, excludeTurns: true });
  st = control(fake, 'oacFake/thread/state', { threadId });
  assert(st.loaded && st.activeTurnId && st.queue.length === 0, 'loading the thread dispatched the queued input');
  const rec = recordedQueueKinds('cases-reload', (p) => p.method === 'thread/resume');
  eq(queueKinds(reload.frames.slice(before)), rec.kinds, `frame order against ${fixturePath('s3')}:${rec.req.line}`);
  eq(reload.notifications('item/started').at(-1).params.item.clientId, 'cid-unloaded', 'clientId = clientUserMessageId');
});

await test('interrupt: turn/completed of an interrupted turn has the recorded shape (S3 capture)', () => {
  const fake = new FakeCodexAppServer();
  const { tui, threadId } = idleThread(fake);
  tui.ok('turn/start', { threadId, input: [{ type: 'text', text: 'busy' }] });
  control(fake, 'oacFake/turn/complete', { threadId, status: 'interrupted', agentText: 'partial' });
  const got = tui.notifications('turn/completed').at(-1);
  const want = readTranscript('s3').find((f) => f.payload.method === 'turn/completed' && f.payload.params.turn.status === 'interrupted');
  eq(shape(got), shape(want.payload), `shape against ${fixturePath('s3')}:${want.line}`);
  eq([got.params.turn.items, got.params.turn.itemsView], [[], 'notLoaded'], 'no items, as recorded');
});

// #343 review finding 2: the interrupted turn's frames, from its agent item/started to its
// turn/completed (S3 L899-L907), as kinds. Live Codex never completes the interrupted agent
// message: an item/completed there must fail this case.
await test('interrupt: the interrupted turn sends the recorded frames in order, and no item/completed for its agent message (S3 capture)', () => {
  const fake = new FakeCodexAppServer();
  const { tui, threadId } = idleThread(fake);
  tui.ok('turn/start', { threadId, input: [{ type: 'text', text: 'busy' }] });
  const before = tui.frames.length;
  control(fake, 'oacFake/turn/complete', { threadId, status: 'interrupted', agentText: 'partial' });
  const kinds = (frames) =>
    frames
      .filter((p) => p.method && !OMITTED.has(p.method) && !(p.params?.item && p.params.item.type !== 'agentMessage'))
      .map((p) => `${p.method}${p.params?.item ? ` ${p.params.item.type}` : ''}${p.params?.status?.type ? ` ${p.params.status.type}` : ''}`);
  const rec = readTranscript('s3').filter((f) => f.mode === 'cases-main' && f.direction === 'daemon->client');
  const start = rec.findIndex((f) => f.line === 899);
  const end = rec.findIndex((f) => f.line === 907);
  assert(start >= 0 && end > start && rec[start].payload.params?.item?.type === 'agentMessage' && rec[end].payload.params?.turn?.status === 'interrupted', 'S3 L899-L907 is the interrupted turn');
  const want = kinds(rec.slice(start, end + 1).map((f) => f.payload));
  eq(kinds(tui.frames.slice(before)), want, `frame kinds against ${fixturePath('s3')}:899-907`);
  assert(!tui.frames.slice(before).some((p) => p.method === 'item/completed'), 'no item/completed for the interrupted agent message');
});

await test('queue: several adds dispatch one per idle, in order', () => {
  const fake = new FakeCodexAppServer();
  const { tui, threadId } = idleThread(fake);
  tui.ok('turn/start', { threadId, input: [{ type: 'text', text: 'busy' }] });
  const adapter = client(fake, 'adapter');
  add(adapter, threadId, 'a');
  add(adapter, threadId, 'b');
  control(fake, 'oacFake/turn/complete', { threadId });
  eq(control(fake, 'oacFake/thread/state', { threadId }).queue.map((q) => q.clientUserMessageId), ['cid-b'], 'one dispatched');
  control(fake, 'oacFake/turn/complete', { threadId });
  eq(control(fake, 'oacFake/thread/state', { threadId }).queue, [], 'second dispatched');
});

// Both halves are recorded: items already queued when the turn is interrupted (S3
// queued-interrupt capture, 2026-10-08) and an add made after it (S3 capture, 2026-10-07).
// The recordings are checked first, so the case cannot pass on a fixture that shows otherwise.
await test('interrupt: a queued item and a later add both wait until a turn completes uninterrupted (both halves recorded, S3 captures)', () => {
  const quietAfterInterrupt = (key) => {
    const fr = readTranscript(key).filter((f) => f.mode === 'cases-main');
    const done = fr.findIndex((f) => f.payload.method === 'turn/completed' && f.payload.params.turn.status === 'interrupted');
    const nextStart = fr.findIndex((f, i) => i > done && f.direction === 'client->daemon' && f.payload.method === 'turn/start');
    assert(done >= 0 && nextStart > done, `${fixturePath(key)}: an interrupted turn followed by a client turn/start`);
    const between = fr.slice(done + 1, nextStart);
    return { quiet: !between.some((f) => f.payload.method === 'turn/started'), adds: between.filter((f) => f.payload.method === 'thread/queue/add').length };
  };
  const qi = quietAfterInterrupt('s3QueuedInterrupt');
  const later = quietAfterInterrupt('s3');
  assert(qi.quiet && qi.adds === 0, 'recorded: the items queued before the interrupt did not start a turn before the next turn/start');
  assert(later.quiet && later.adds === 1, 'recorded: the add after the interrupt did not start a turn before the next turn/start');
  const fake = new FakeCodexAppServer();
  const { tui, threadId } = idleThread(fake);
  tui.ok('turn/start', { threadId, input: [{ type: 'text', text: 'busy' }] });
  const adapter = client(fake, 'adapter');
  add(adapter, threadId, 'a');
  const r = control(fake, 'oacFake/turn/complete', { threadId, status: 'interrupted' });
  eq(r.dispatched, null, 'nothing dispatched after an interrupt');
  eq(tui.notifications('turn/completed').at(-1).params.turn.status, 'interrupted', 'turn/completed status');
  add(adapter, threadId, 'b');
  let st = control(fake, 'oacFake/thread/state', { threadId });
  eq([st.activeTurnId, st.queue.length, st.lastTurnInterrupted], [null, 2, true], 'idle-after-interrupt add waits');
  tui.ok('turn/start', { threadId, input: [{ type: 'text', text: 'user again' }] });
  control(fake, 'oacFake/turn/complete', { threadId });
  st = control(fake, 'oacFake/thread/state', { threadId });
  eq(st.queue.map((q) => q.clientUserMessageId), ['cid-b'], 'head dispatched after an uninterrupted turn');
});

await test('refusals: ephemeral, archived and missing thread as recorded (S3 capture); nothing unrecorded is answered as Codex', () => {
  const fake = new FakeCodexAppServer();
  const c = client(fake, 'adapter');
  const mk = (p) => control(fake, 'oacFake/thread/create', p).threadId;
  // The recorded answers, with the recorded thread id replaced by this call's.
  const s3 = readTranscript('s3');
  const recordedAnswer = (words) => {
    const req = s3.find((f) => f.direction === 'client->daemon' && f.payload.method === 'thread/queue/add' && JSON.stringify(f.payload.params.input).includes(words));
    const resp = s3.find((f) => f.line > req.line && f.mode === req.mode && f.payload.id === req.payload.id && f.payload.error);
    return (id) => JSON.parse(JSON.stringify(resp.payload.error).split(req.payload.params.threadId).join(id));
  };
  const cases = [
    [mk({ ephemeral: true }), recordedAnswer('OAC S3 EPHEMERAL')],
    [mk({ archived: true, loaded: false }), recordedAnswer('OAC S3 ARCHIVED')],
    ['01a0e550-1921-7000-93ef-000000000000', recordedAnswer('OAC S3 UNKNOWN')],
  ];
  for (const [id, want] of cases) eq(add(c, id).error, want(id), `refusal for ${id}`);
  // Live Codex answers an unknown thread -32603 from its thread store, not the -32600
  // "thread not found" of thread_queue_processor.rs (S3 capture, #343).
  eq(add(c, '01a0e550-1921-7000-93ef-000000000001').error.code, -32603, 'unknown thread is an internal error');
  // Gate S3 criterion 5: the subagent refusals and "no queue service" are recorded nowhere,
  // so the fake gives no Codex answer for either. A subagent thread, loaded or not, is
  // answered NOT_MODELLED, and so is every list that would show it.
  for (const p of [{ subagent: 'multi-agent-v2' }, { subagent: 'thread-spawn', loaded: false }, { subagent: 'thread-spawn' }, { subagent: 'multi-agent-v2', loaded: false }]) {
    const e = add(c, mk(p)).error;
    eq([e.code, e.data], [NOT_MODELLED, { oacFake: 'not-modelled', method: 'thread/queue/add' }], `subagent ${JSON.stringify(p)}`);
  }
  eq(c.request('thread/loaded/list', {}).error.code, NOT_MODELLED, 'thread/loaded/list with a subagent thread');
  eq(c.request('thread/list', { cursor: null, limit: 5, sortKey: 'created_at' }).error.code, NOT_MODELLED, 'thread/list with a subagent thread');
  // The queue-unavailable switch is gone: no control can make a host without a queue service.
  const ctl = client(fake, 'oac-fake-control', { init: false });
  eq(ctl.request('oacFake/queue/setAvailable', { available: false }).error.code, -32601, 'oacFake/queue/setAvailable is not a control');
  eq(control(fake, 'oacFake/calls', { clientName: 'adapter' }).calls.filter((x) => !['thread/queue/add', 'thread/loaded/list', 'thread/list'].includes(x.method)).map((x) => x.method), ['initialize', 'initialized'], 'no fallback call was made by the fake');
});

await test('thread/create makes only recorded states: unrecorded combinations are refused, and no add is accepted in one', () => {
  const fake = new FakeCodexAppServer();
  const c = client(fake, 'adapter');
  const ctl = client(fake, 'oac-fake-control', { init: false });
  const create = (p) => ctl.request('oacFake/thread/create', p);
  // Refused at create: no fixture records these states.
  // Each is refused by its own guard: the reason names it.
  const refused = [
    [{ archived: true }, 'archived'], // archived but loaded: the S3 capture's archived thread was notLoaded (L1024-L1026)
    [{ archived: true, loaded: true }, 'archived'],
    [{ archived: true, loaded: false, materialized: false }, 'archived'],
    [{ archived: true, loaded: false, ephemeral: true }, 'archived'],
    [{ ephemeral: true, loaded: false }, 'ephemeral'], // the recorded ephemeral refusal is for a loaded thread (L111-L119)
    [{ ephemeral: true, materialized: true }, 'ephemeral'],
    [{ loaded: false, materialized: false }, 'not loaded'],
    [{ subagent: 'multi-agent-v2', ephemeral: true }, 'subagent'],
    [{ subagent: 'thread-spawn', archived: true, loaded: false }, 'subagent'],
  ];
  for (const [p, why] of refused) {
    const e = create(p).error;
    eq(e?.code, -32602, `create ${JSON.stringify(p)} is refused`);
    assert(e.message.includes(why), `create ${JSON.stringify(p)} is refused for the ${why} guard: ${e.message}`);
  }
  eq(fake.threads.size, 0, 'no thread was made');
  // The recorded states are made, and their adds answer as recorded.
  const mk = (p) => create(p).result.threadId;
  const arch = mk({ archived: true, loaded: false });
  eq(add(c, arch).error.code, -32600, 'archived add refused (S3 L1030-L1032)');
  const eph = mk({ ephemeral: true });
  eq(control(fake, 'oacFake/thread/state', { threadId: eph }).loaded, true, 'an ephemeral thread is loaded');
  eq(add(c, eph).error.code, -32600, 'ephemeral add refused (S3 L114-L119)');
  // An archived thread refuses the add whether or not it is loaded (the handler does not
  // depend on create's guard).
  fake.threads.get(arch).loaded = true;
  eq(add(c, arch).error.code, -32600, 'archived and loaded: still the archived refusal');
  // A thread before its first turn (thread/start, no turn): no fixture records an add to one.
  const fresh = c.ok('thread/start', {}).thread.id;
  eq(add(c, fresh).error.code, NOT_MODELLED, 'an add before the first turn is NOT_MODELLED');
  eq(control(fake, 'oacFake/thread/state', { threadId: fresh }).queue.length, 0, 'nothing queued');
  // No fixture records a list taken while an ephemeral or archived thread exists.
  eq(c.request('thread/list', { cursor: null, limit: 5, sortKey: 'created_at' }).error.code, NOT_MODELLED, 'thread/list with an ephemeral or archived thread');
  eq(c.request('thread/loaded/list', {}).error.code, NOT_MODELLED, 'thread/loaded/list with a loaded ephemeral thread');
});

await test('thread/turns/list: an empty list, and a list on a thread with unmodelled earlier turns, answer NOT_MODELLED', () => {
  const fake = new FakeCodexAppServer();
  const c = client(fake, 'adapter');
  const list = (threadId) => c.request('thread/turns/list', { threadId, limit: 5, sortDirection: 'desc', itemsView: 'full' });
  // Every recorded thread/turns/list result has at least one turn.
  const fresh = c.ok('thread/start', {}).thread.id;
  eq(list(fresh).error?.code, NOT_MODELLED, 'thread/start, then thread/turns/list before any turn');
  // oacFake/thread/create {} makes a materialized thread: one that has run turns the fake
  // never saw. Listing its turns would leave them out, before and after a new turn.
  const made = control(fake, 'oacFake/thread/create', {}).threadId;
  eq(list(made).error?.code, NOT_MODELLED, 'oacFake/thread/create {}, then thread/turns/list');
  assert(add(c, made, 'later').result, 'an add to it is accepted and starts a turn');
  eq(list(made).error?.code, NOT_MODELLED, 'still not listed after a new turn');
  // A thread whose every turn the fake ran lists them (G2).
  const { threadId } = idleThread(fake);
  eq(list(threadId).result?.data?.length, 1, 'thread/start and one turn: one turn listed');
});

await test('preview: a thread made materialized by thread/create lists and resumes with a non-empty preview, kept after a later turn', () => {
  const fake = new FakeCodexAppServer();
  const c = client(fake, 'adapter');
  // Every recorded thread/list row and thread/resume result has a non-empty preview, the
  // thread's first user message (S3 L42, L50); "" is recorded only from thread/start.
  const threadId = control(fake, 'oacFake/thread/create', {}).threadId;
  const listed = () => c.ok('thread/list', { cursor: null, limit: 5, sortKey: 'created_at' }).data.find((t) => t.id === threadId);
  const before = listed()?.preview;
  assert(typeof before === 'string' && before.length > 0, `thread/list preview is non-empty: ${JSON.stringify(before)}`);
  eq(before, PRIOR_TURNS_PREVIEW, 'the fake stand-in text');
  const resumed = c.ok('thread/resume', { threadId, excludeTurns: true }).thread.preview;
  eq(resumed, before, 'thread/resume preview');
  assert(add(c, threadId, 'a later turn').result, 'an add starts a later turn');
  control(fake, 'oacFake/turn/complete', { threadId });
  eq(listed().preview, before, 'a later turn leaves the preview as it was');
  eq(c.ok('thread/resume', { threadId, excludeTurns: true }).thread.preview, before, 'resumed after the later turn');
});

await test('archive control: a thread put into the recorded archived state gets the recorded refusal, and takes adds again once put back', () => {
  const fake = new FakeCodexAppServer();
  const { threadId } = idleThread(fake);
  const c = client(fake, 'adapter');
  const s3 = readTranscript('s3');
  const req = s3.find((f) => f.direction === 'client->daemon' && f.payload.method === 'thread/queue/add' && JSON.stringify(f.payload.params.input).includes('OAC S3 ARCHIVED'));
  const resp = s3.find((f) => f.line > req.line && f.mode === req.mode && f.payload.id === req.payload.id && f.payload.error);
  const want = JSON.parse(JSON.stringify(resp.payload.error).split(req.payload.params.threadId).join(threadId));
  eq(control(fake, 'oacFake/thread/setArchived', { threadId, archived: true }), { threadId, archived: true, loaded: false }, 'archived');
  const before = c.frames.length;
  eq(add(c, threadId, 'refused').error, want, 'the recorded archived refusal (S3 L1030-L1032)');
  eq(c.frames.length, before + 1, 'the refusal is the only frame');
  let st = control(fake, 'oacFake/thread/state', { threadId });
  eq([st.loaded, st.activeTurnId, st.queue.length], [false, null, 0], 'nothing queued or started');
  eq(control(fake, 'oacFake/thread/setArchived', { threadId, archived: false }), { threadId, archived: false, loaded: true }, 'put back');
  assert(add(c, threadId, 'later').result, 'an add is accepted again');
  st = control(fake, 'oacFake/thread/state', { threadId });
  assert(st.activeTurnId, 'the later add started a turn at once (S3 L65-L73)');
  const ctl = client(fake, 'oac-fake-control', { init: false });
  eq(ctl.request('oacFake/thread/setArchived', { threadId, archived: true }).error.code, -32602, 'a thread with a running turn is not archived');
  const eph = control(fake, 'oacFake/thread/create', { ephemeral: true }).threadId;
  eq(ctl.request('oacFake/thread/setArchived', { threadId: eph, archived: true }).error.code, -32602, 'an ephemeral thread is not archived');
});

await test('steering: turn/steer is refused NOT_MODELLED and flagged; turn/start is flagged, and steers a running turn', () => {
  const fake = new FakeCodexAppServer();
  const { tui, threadId } = idleThread(fake);
  const adapter = client(fake, 'adapter');
  const s = adapter.request('turn/steer', { threadId, input: [{ type: 'text', text: 'x' }] });
  eq(s.error.code, NOT_MODELLED, 'turn/steer error code');
  tui.ok('turn/start', { threadId, input: [{ type: 'text', text: 'busy' }] });
  const running = control(fake, 'oacFake/thread/state', { threadId }).activeTurnId;
  const r = adapter.ok('turn/start', { threadId, input: [{ type: 'text', text: 'joins' }] });
  eq(r.turn.id, running, 'busy turn/start returns the running turn id (G5 L58)');
  control(fake, 'oacFake/turn/complete', { threadId });
  const turn = control(fake, 'oacFake/thread/state', { threadId }).turns.at(-1);
  eq(turn.items.filter((i) => i.type === 'userMessage').map((i) => i.content[0].text), ['busy', 'joins'], 'input joined the running turn (G5 L66)');
  const log = control(fake, 'oacFake/calls', { clientName: 'adapter' });
  eq(log.steering.length, 2, 'two steering calls flagged');
  eq(log.calls.filter((x) => x.flags.steered).length, 1, 'one call actually steered');
});

await test('overrides: thread/queue/add members beyond threadId, input, clientUserMessageId are flagged', () => {
  const fake = new FakeCodexAppServer();
  const { threadId } = idleThread(fake);
  const adapter = client(fake, 'adapter');
  add(adapter, threadId, 'x', { model: 'm', approvalPolicy: 'never' });
  const log = control(fake, 'oacFake/calls', { clientName: 'adapter' });
  eq(log.calls.find((c) => c.seq === log.overrideMembers[0]).flags.overrideMembers, ['model', 'approvalPolicy'], 'flagged members');
  const clean = add(adapter, threadId, 'y');
  assert(clean.result, 'a clean add succeeds');
  eq(log.overrideMembers.length, 1, 'only the first add is flagged');
});

await test('hand-off flag: exactly the thread/queue/add, turn/start and turn/steer calls are hand-offs', () => {
  const fake = new FakeCodexAppServer();
  const { threadId } = idleThread(fake);
  const adapter = client(fake, 'adapter');
  adapter.ok('thread/loaded/list', {});
  add(adapter, threadId, 'a');
  control(fake, 'oacFake/turn/complete', { threadId });
  adapter.request('turn/steer', { threadId, input: [{ type: 'text', text: 's' }] });
  adapter.ok('turn/start', { threadId, input: [{ type: 'text', text: 't' }] });
  adapter.ok('thread/turns/list', { threadId, limit: 5, sortDirection: 'desc', itemsView: 'full' });
  const log = control(fake, 'oacFake/calls', { clientName: 'adapter' });
  const bySeq = new Map(log.calls.map((c) => [c.seq, c.method]));
  eq(log.handOffs.map((s) => bySeq.get(s)), ['thread/queue/add', 'turn/steer', 'turn/start'], 'hand-off calls');
});

await test('robustness: non-object params are logged and answered NOT_MODELLED, and the fake keeps serving', () => {
  const fake = new FakeCodexAppServer();
  const { threadId } = idleThread(fake);
  const adapter = client(fake, 'adapter');
  for (const [i, params] of [null, [], 'x', 7].entries()) {
    adapter.conn.receive({ jsonrpc: '2.0', id: 100 + i, method: 'thread/start', params });
    const r = adapter.frames.find((f) => f.id === 100 + i);
    eq([r.error?.code, r.error?.data?.oacFake], [NOT_MODELLED, 'not-modelled'], `params ${JSON.stringify(params)}`);
  }
  adapter.conn.receive({ jsonrpc: '2.0', id: 200, method: 'turn/steer', params: null });
  adapter.conn.receive({ jsonrpc: '2.0', method: 'initialized', params: null });
  const log = control(fake, 'oacFake/calls', { clientName: 'adapter' });
  eq(log.calls.filter((c) => c.flags.malformedParams).length, 6, 'four requests, the turn/steer and the notification logged as malformed params');
  assert(log.calls.find((c) => c.id === 200).flags.steering, 'a turn/steer with null params is still flagged as steering');
  assert(add(adapter, threadId, 'after').result, 'still serving');
});

await test('robustness: batches, non-object and unparseable frames are logged; steering inside a batch is flagged', () => {
  const fake = new FakeCodexAppServer();
  const { threadId } = idleThread(fake);
  const adapter = client(fake, 'adapter');
  const before = control(fake, 'oacFake/thread/state', { threadId });
  adapter.conn.receive([
    { jsonrpc: '2.0', id: 1, method: 'turn/steer', params: { threadId, input: [{ type: 'text', text: 'b1' }] } },
    { jsonrpc: '2.0', id: 2, method: 'turn/start', params: { threadId, input: [{ type: 'text', text: 'b2' }] } },
    { jsonrpc: '2.0', id: 3, method: 'thread/queue/add', params: { threadId, clientUserMessageId: 'b3', input: [{ type: 'text', text: 'b3' }] } },
    42,
  ]);
  adapter.conn.receive(42);
  adapter.conn.receive(null);
  adapter.conn.receiveText('{not json');
  adapter.conn.receive({ jsonrpc: '2.0', id: 9 });
  const errors = adapter.frames.filter((f) => f.error && f.id === null);
  eq(errors.length, 4, 'batch, 42, null and unparseable answered with id null');
  assert(errors.every((e) => e.error.code === NOT_MODELLED), 'with NOT_MODELLED');
  const log = control(fake, 'oacFake/calls', { clientName: 'adapter' });
  eq(log.calls.filter((c) => c.kind === 'malformed').map((c) => c.flags.malformed), ['batch', 'not-an-object', 'not-an-object', 'unparseable', 'no-method'], 'malformed frames logged');
  const batchSeq = log.calls.find((c) => c.flags.malformed === 'batch').seq;
  const members = log.calls.filter((c) => c.flags.inBatch === batchSeq);
  eq(members.map((c) => [c.method, Boolean(c.flags.steering), c.outcome]), [['turn/steer', true, 'error'], ['turn/start', true, 'error'], ['thread/queue/add', false, 'error']], 'batch members');
  eq(log.steering.length, 2, 'both steering members are in the steering list');
  const after = control(fake, 'oacFake/thread/state', { threadId });
  eq([after.activeTurnId, after.queue.length, after.turns.length], [before.activeTurnId, before.queue.length, before.turns.length], 'no batch member was dispatched');
});

await test('control log: oacFake/* calls are logged apart from Codex calls, with their client name', () => {
  const fake = new FakeCodexAppServer();
  const { threadId } = idleThread(fake);
  const adapter = client(fake, 'adapter');
  adapter.request('oacFake/thread/state', { threadId });
  const log = control(fake, 'oacFake/calls', { clientName: 'adapter' });
  eq(log.control.map((c) => [c.method, c.clientName]), [['oacFake/thread/state', 'adapter']], 'adapter control calls');
  eq(log.calls.some((c) => c.method?.startsWith('oacFake/')), false, 'not in the Codex call log');
  eq(control(fake, 'oacFake/calls', {}).control.length > 3, true, 'all control calls are logged');
});

await test('lists: thread/loaded/list, thread/list (no unmaterialized thread), thread/turns/list', () => {
  const fake = new FakeCodexAppServer();
  const c = client(fake, 'tui');
  const fresh = c.ok('thread/start', {}).thread.id;
  const { threadId } = idleThread(fake);
  const loaded = c.ok('thread/loaded/list', {});
  eq([loaded.data.includes(fresh), loaded.data.includes(threadId), loaded.nextCursor], [true, true, null], 'loaded list');
  const listed = c.ok('thread/list', { cursor: null, limit: 5, sortKey: 'created_at' }).data.map((t) => t.id);
  eq([listed.includes(fresh), listed.includes(threadId)], [false, true], 'thread/list omits a thread with no turn yet (G2 L17-L20)');
  const turns = c.ok('thread/turns/list', { threadId, limit: 5, sortDirection: 'desc', itemsView: 'full' }).data;
  eq(turns.map((t) => [t.status, t.itemsView, t.items.map((i) => i.type)]), [['completed', 'full', ['userMessage', 'agentMessage']]], 'turns');
  assert(!('_fixtureNote' in c.ok('thread/list', { cursor: null, limit: 5, sortKey: 'created_at' })), 'redaction note never on the wire');
});

await test('unmodelled methods and parameters answer NOT_MODELLED, never an invented Codex error', () => {
  const fake = new FakeCodexAppServer();
  const { threadId } = idleThread(fake);
  const c = client(fake, 'adapter');
  for (const [m, p] of [
    ['turn/interrupt', { threadId }],
    ['thread/fork', { threadId }],
    ['thread/queue/list', { threadId }],
    ['thread/resume', { threadId }],
    ['turn/start', { threadId, input: [{ type: 'text', text: 'x' }], model: 'm' }],
  ]) {
    const r = c.request(m, p);
    eq([r.error?.code, r.error?.data?.oacFake], [NOT_MODELLED, 'not-modelled'], m);
  }
});

await test('provenance: every fixture the fake reads has a MANIFEST.json entry', () => {
  const manifest = JSON.parse(readFileSync(join(REPO_ROOT, 'docs/planning/gates/fixtures/MANIFEST.json'), 'utf8'));
  const paths = new Set(manifest.fixtures.map((f) => f.path));
  for (const key of Object.keys(FIXTURE_FILES)) assert(paths.has(fixturePath(key)), `${fixturePath(key)} has no MANIFEST entry`);
  const used = new FakeCodexAppServer().templateSources;
  for (const [name, src] of Object.entries(used)) assert(paths.has(src.file), `template ${name} comes from ${src.file}`);
});

// ---- 3. transports --------------------------------------------------------------------

function startServer(args) {
  const p = spawn(process.execPath, [SERVER, ...args], { stdio: ['pipe', 'pipe', 'pipe'] });
  const stderr = createInterface({ input: p.stderr });
  const stdout = createInterface({ input: p.stdout });
  const lines = [];
  const waiters = [];
  stdout.on('line', (l) => {
    lines.push(JSON.parse(l));
    waiters.splice(0).forEach((w) => w());
  });
  const url = new Promise((resolve, reject) => {
    stderr.once('line', (l) => {
      try {
        resolve(JSON.parse(l).url);
      } catch {
        reject(new Error(`unexpected stderr: ${l}`));
      }
    });
    p.once('exit', (code) => reject(new Error(`server exited ${code}`)));
  });
  const nextStdout = (pred) =>
    new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error('timed out waiting for a stdout frame')), 5000);
      const check = () => {
        const f = lines.find(pred);
        if (f) {
          clearTimeout(timer);
          resolve(f);
        } else waiters.push(check);
      };
      check();
    });
  return { p, url, nextStdout, write: (o) => p.stdin.write(`${JSON.stringify(o)}\n`) };
}
function wsRequests(ch) {
  const frames = [];
  const waiters = [];
  ch.on('message', (t) => {
    frames.push(JSON.parse(t));
    waiters.splice(0).forEach((w) => w());
  });
  let id = 0;
  return {
    frames,
    request(method, params = {}) {
      const myId = id++;
      ch.send(JSON.stringify({ jsonrpc: '2.0', id: myId, method, params }));
      return new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error(`timed out waiting for ${method}`)), 5000);
        const check = () => {
          const f = frames.find((x) => x.id === myId && x.method === undefined);
          if (f) {
            clearTimeout(timer);
            resolve(f);
          } else waiters.push(check);
        };
        check();
      });
    },
  };
}

await test('transports: stdio JSONL and a loopback WebSocket share one fake; stdin EOF exits 0', async () => {
  const s = startServer(['--stdio', '--listen', 'ws://127.0.0.1:0']);
  try {
    const url = await s.url;
    assert(/^ws:\/\/127\.0\.0\.1:\d+$/.test(url), `listen url ${url}`);
    s.write({ jsonrpc: '2.0', id: 0, method: 'initialize', params: { clientInfo: { name: 'adapter', version: '1' }, capabilities: { experimentalApi: true } } });
    const init = await s.nextStdout((f) => f.id === 0);
    assert(init.result.userAgent.endsWith('(adapter; 1)'), `userAgent ${init.result.userAgent}`);
    await s.nextStdout((f) => f.method === 'remoteControl/status/changed');
    s.write({ jsonrpc: '2.0', method: 'initialized', params: {} });
    // A malformed request must not take the process down (review item 1).
    s.write({ jsonrpc: '2.0', id: 50, method: 'thread/start', params: null });
    eq((await s.nextStdout((f) => f.id === 50)).error.code, NOT_MODELLED, 'params null answered');
    s.p.stdin.write('[{"jsonrpc":"2.0","id":51,"method":"turn/steer","params":null}]\nnot json\n');
    s.write({ jsonrpc: '2.0', id: 1, method: 'thread/start', params: {} });
    const threadId = (await s.nextStdout((f) => f.id === 1)).result.thread.id;

    const ch = await connectWs(url);
    assert(/x-codex-websocket-max-unfragmented-message-bytes: 16777216/i.test(ch.handshake), 'recorded handshake header');
    const ws = wsRequests(ch);
    await ws.request('initialize', { clientInfo: { name: 'tui', version: '1' }, capabilities: {} });
    ch.send(JSON.stringify({ jsonrpc: '2.0', method: 'initialized', params: {} }));
    const t = await ws.request('turn/start', { threadId, input: [{ type: 'text', text: 'over ws' }] });
    assert(t.result.turn.status === 'inProgress', 'turn started over ws');
    await s.nextStdout((f) => f.method === 'turn/started');
    const big = 'x'.repeat(70000); // a 64-bit-length frame
    s.write({ jsonrpc: '2.0', id: 2, method: 'thread/queue/add', params: { threadId, clientUserMessageId: 'c1', input: [{ type: 'text', text: big }] } });
    const q = await s.nextStdout((f) => f.id === 2);
    assert(q.result.queuedSubmission.input[0].text.length === 70000, 'large input queued');
    await ws.request('oacFake/turn/complete', { threadId, agentText: 'done' });
    const dispatched = await s.nextStdout((f) => f.method === 'item/started' && f.params.item.clientId === 'c1');
    assert(dispatched.params.item.content[0].text === big, 'dispatched input over stdio');
    const calls = (await ws.request('oacFake/calls', { clientName: 'adapter' })).result;
    eq(
      calls.calls.map((c) => [c.transport, c.method ?? c.flags.malformed]),
      [['stdio', 'initialize'], ['stdio', 'initialized'], ['stdio', 'thread/start'], ['stdio', 'batch'], ['stdio', 'turn/steer'], ['stdio', 'unparseable'], ['stdio', 'thread/start'], ['stdio', 'thread/queue/add']],
      'stdio call log',
    );
    eq(calls.steering.length, 1, 'the batched turn/steer is flagged');
    ch.close();
    s.p.stdin.end();
    const code = await new Promise((r) => s.p.once('exit', r));
    eq(code, 0, 'exit code');
  } finally {
    s.p.kill();
  }
});

await test('transports: a non-loopback or name-resolved --listen address is refused (0.0.0.0, localhost)', async () => {
  for (const url of ['ws://0.0.0.0:0', 'ws://localhost:0']) {
    const p = spawn(process.execPath, [SERVER, '--listen', url], { stdio: ['ignore', 'ignore', 'pipe'] });
    let err = '';
    p.stderr.on('data', (d) => (err += d));
    const code = await new Promise((r) => p.once('exit', r));
    assert(code !== 0 && /non-loopback/.test(err), `${url}: exit ${code}: ${err}`);
  }
});

await test('transports: a WebSocket frame over the 16 MiB cap is refused (close 1009) and logged, unbuffered', async () => {
  const s = startServer(['--listen', 'ws://127.0.0.1:0']);
  try {
    const url = await s.url;
    const ch = await connectWs(url);
    const got = [];
    ch.on('message', (t) => got.push(JSON.parse(t)));
    const closed = new Promise((r) => ch.on('close', r));
    const rawClose = new Promise((r) => {
      let b = Buffer.alloc(0);
      ch.socket.on('data', (d) => {
        b = Buffer.concat([b, d]);
        const i = b.indexOf(Buffer.from([0x88, 0x02]));
        if (i >= 0 && b.length >= i + 4) r(b.readUInt16BE(i + 2));
      });
    });
    const header = Buffer.alloc(14);
    header[0] = 0x81;
    header[1] = 0x80 | 127;
    header.writeBigUInt64BE(BigInt(16777217), 2); // declared length only; no payload follows
    ch.socket.write(header);
    eq(await rawClose, 1009, 'close code');
    await closed;
    eq(got.map((f) => [f.id, f.error?.code]), [[null, NOT_MODELLED]], 'error frame before the close');
    const ctl = wsRequests(await connectWs(url));
    const log = (await ctl.request('oacFake/calls', {})).result;
    eq(log.calls.map((c) => c.flags.malformed), ['too-large'], 'logged as too-large');
  } finally {
    s.p.kill();
  }
});

for (const r of results) console.log(r);
console.log(`fake Codex app-server self-test: ${passed}/${passed + failed} cases pass`);
process.exitCode = failed ? 1 : 0;
