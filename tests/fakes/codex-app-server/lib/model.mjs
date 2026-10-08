// SPDX-License-Identifier: Apache-2.0
//
// The fake Codex app-server's protocol model (#58, F9): connections, threads, turns, the
// user-message queue, and a call log for test assertions. Transport-free: a connection is a
// `send(obj)` callback, and `receive(obj)` takes one decoded JSON-RPC message. server.mjs
// binds it to stdio (JSONL) and to a loopback WebSocket listener; tests may also use it
// in-process.
//
// Rules for this file (README "Provenance" holds the per-behaviour table):
//   - Wire shapes come from lib/fixtures.mjs templates, which are recorded frames. Code here
//     only substitutes ids, text and times into them.
//   - A behaviour with no recorded fixture is answered with the fake's own NOT_MODELLED
//     error (Gate S3 criterion 5: every fake behaviour traces to a recorded fixture). The
//     fake never invents a Codex error message or a Codex frame shape.
//   - Turns never progress on a timer. A turn runs until a test calls
//     `oacFake/turn/complete` (no sleeps, no polling, deterministic order).

import { randomBytes } from 'node:crypto';
import { loadTemplates } from './fixtures.mjs';

// JSON-RPC codes in the implementation-defined server-error range, used only by the fake.
export const NOT_MODELLED = -32099;
export const FAKE_INTERNAL = -32098;
const isPlainObject = (v) => v !== null && typeof v === 'object' && !Array.isArray(v);
export const CONTROL_PREFIX = 'oacFake/';

// Every thread/queue/add refusal the fake gives is replayed from a recorded frame (#343, the
// S3 capture: README "Recorded behaviours"). The subagent refusals and "no queue service"
// are recorded nowhere, so a subagent thread is answered NOT_MODELLED and the fake has no
// queue-less host (README "Not modelled").
const EXPERIMENTAL_METHODS = new Set(['thread/queue/add']);
const QUEUE_ADD_MEMBERS = new Set(['threadId', 'input', 'clientUserMessageId']);
const TURN_START_MEMBERS = new Set(['threadId', 'input']);
// Methods that hand input to a thread: every call is flagged in the call log.
const HANDOFF_METHODS = new Set(['thread/queue/add', 'turn/start', 'turn/steer']);
// Methods [SEC-AUZ-022] / spec/bindings/mcp.md 8.2.1 classify as steering operations.
const STEERING_METHODS = new Set(['turn/steer', 'turn/start']);

export const SUBAGENT_KINDS = ['multi-agent-v2', 'thread-spawn'];

export function uuidv7(nowMs = Date.now()) {
  const b = randomBytes(16);
  b.writeUIntBE(nowMs, 0, 6);
  b[6] = (b[6] & 0x0f) | 0x70;
  b[8] = (b[8] & 0x3f) | 0x80;
  const h = b.toString('hex');
  return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`;
}
const agentItemId = () => `msg_${randomBytes(25).toString('hex')}`;
const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

class RpcError extends Error {
  constructor(code, message, data) {
    super(message);
    this.code = code;
    this.data = data;
  }
}
const notModelled = (method, why) =>
  new RpcError(NOT_MODELLED, `oac fake Codex app-server: ${method}: not modelled (${why}); see tests/fakes/codex-app-server/README.md`, {
    oacFake: 'not-modelled',
    method,
  });

const textInput = (method, input) => {
  if (!Array.isArray(input) || input.length === 0 || !input.every((i) => i && i.type === 'text' && typeof i.text === 'string')) {
    throw notModelled(method, 'only a non-empty input array of {type:"text", text} items is recorded');
  }
  return input.map((i) => ({ type: 'text', text: i.text }));
};

export class FakeCodexAppServer {
  constructor({ templates, now = Date.now } = {}) {
    const loaded = templates ? { templates } : loadTemplates();
    this.t = loaded.templates;
    this.templateSources = loaded.used ?? {};
    this.now = now;
    this.connections = new Set();
    this.threads = new Map();
    this.calls = [];
    this.controlCalls = [];
    this.nextConnId = 1;
  }

  // ---- connections -------------------------------------------------------------------

  connect(send, { transport = 'in-process' } = {}) {
    const conn = {
      id: this.nextConnId++,
      transport,
      send,
      initialized: false,
      experimentalApi: false,
      clientName: null,
      open: true,
    };
    this.connections.add(conn);
    return {
      id: conn.id,
      receive: (msg) => this.receive(conn, msg),
      receiveText: (text) => this.receiveText(conn, text),
      // A frame the transport refused for its size: logged, answered, never parsed.
      tooLarge: (bytes) => {
        const e = this.logMalformed(conn, 'too-large', { bytes });
        this.reply(conn, null, e, notModelled('(frame)', `a frame of ${bytes} bytes is over the fake's size cap`));
      },
      close: () => {
        conn.open = false;
        this.connections.delete(conn);
        for (const th of this.threads.values()) th.subscribers.delete(conn);
      },
    };
  }

  out(conn, obj) {
    if (conn.open) conn.send(obj);
  }
  // A recorded error answer, with the recorded thread id replaced by this call's.
  recordedError(err, recordedThreadId = null, threadId = null) {
    const message = recordedThreadId === null ? err.message : err.message.split(recordedThreadId).join(String(threadId));
    return new RpcError(err.code, message);
  }
  notify(conn, template, params) {
    this.out(conn, { method: template.method, params, emittedAtMs: this.now() });
  }
  broadcast(thread, template, params) {
    for (const c of thread.subscribers) this.notify(c, template, params);
  }

  // One frame as text, as a transport received it. A frame that is not JSON is logged
  // (flags.malformed "unparseable") and answered with the fake's own error.
  receiveText(conn, text) {
    let msg;
    try {
      msg = JSON.parse(text);
    } catch {
      const e = this.logMalformed(conn, 'unparseable', String(text));
      this.reply(conn, null, e, notModelled('(frame)', 'a frame that is not JSON is not recorded'));
      return;
    }
    this.receive(conn, msg);
  }

  // One decoded frame. Nothing a client sends can crash the fake: every frame is logged
  // before anything else, and every frame the fake cannot model gets its own error.
  receive(conn, msg) {
    if (Array.isArray(msg)) {
      // A JSON-RPC batch: no fixture records one, so no member is dispatched. Each member
      // that names a method is still logged, so a turn/steer or turn/start inside a batch
      // is flagged as steering like any other.
      const batch = this.logMalformed(conn, 'batch', msg);
      for (const m of msg) {
        if (isPlainObject(m) && typeof m.method === 'string' && !m.method.startsWith(CONTROL_PREFIX)) {
          const e = this.logCall(conn, m);
          e.flags.inBatch = batch.seq;
          e.outcome = 'error';
          e.error = { code: NOT_MODELLED, message: 'not dispatched: inside a JSON-RPC batch' };
        }
      }
      this.reply(conn, null, batch, notModelled('(batch)', 'JSON-RPC batches are not recorded; no member was dispatched'));
      return;
    }
    if (!isPlainObject(msg)) {
      const e = this.logMalformed(conn, 'not-an-object', msg);
      this.reply(conn, null, e, notModelled('(frame)', 'a frame that is not a JSON object is not recorded'));
      return;
    }
    if (typeof msg.method !== 'string') {
      // A client response (to a server request; none is modelled) or a malformed frame.
      const isResponse = msg.id !== undefined && ('result' in msg || 'error' in msg);
      const e = this.logMalformed(conn, isResponse ? 'unsolicited-response' : 'no-method', msg);
      if (!isResponse && msg.id !== undefined) this.reply(conn, msg.id, e, notModelled('(frame)', 'a request with no method is not recorded'));
      return;
    }
    const { method } = msg;
    const params = msg.params === undefined ? {} : msg.params;
    const isRequest = msg.id !== undefined;
    const control = method.startsWith(CONTROL_PREFIX);
    const entry = control ? this.logControl(conn, msg) : this.logCall(conn, msg);
    try {
      if (!isPlainObject(params)) {
        entry.flags.malformedParams = true;
        throw notModelled(method, 'params that are not a JSON object are not recorded');
      }
      const result = control ? this.control(method, params) : this.dispatch(conn, method, params, isRequest, entry);
      if (isRequest && result !== undefined) {
        entry.outcome = 'result';
        this.out(conn, { id: msg.id, result: result.result });
        result.after?.();
      }
    } catch (e) {
      let err = e;
      if (!(e instanceof RpcError)) {
        process.stderr.write(`oac fake codex: internal error on ${method}: ${e?.stack ?? e}\n`);
        err = new RpcError(FAKE_INTERNAL, `oac fake Codex app-server: internal error in the fake on ${method}`, { oacFake: 'internal-error', method });
      }
      this.reply(conn, isRequest ? msg.id : undefined, entry, err);
    }
  }

  // Record the error on the log entry, and answer it when the frame had an id (or null).
  reply(conn, id, entry, err) {
    entry.outcome = 'error';
    entry.error = { code: err.code, message: err.message };
    if (id !== undefined) this.out(conn, { error: { code: err.code, message: err.message, ...(err.data ? { data: err.data } : {}) }, id });
  }

  logMalformed(conn, reason, frame) {
    const entry = {
      seq: this.calls.length + 1,
      connection: conn.id,
      transport: conn.transport,
      clientName: conn.clientName,
      kind: 'malformed',
      method: null,
      id: null,
      params: null,
      frame: structuredClone(frame),
      flags: { malformed: reason },
      outcome: 'logged',
    };
    this.calls.push(entry);
    return entry;
  }

  logControl(conn, msg) {
    const entry = {
      seq: this.controlCalls.length + 1,
      connection: conn.id,
      transport: conn.transport,
      clientName: conn.clientName,
      method: msg.method,
      id: msg.id ?? null,
      params: structuredClone(msg.params ?? {}),
      flags: {},
      outcome: msg.id !== undefined ? 'pending' : 'notification',
    };
    this.controlCalls.push(entry);
    return entry;
  }

  logCall(conn, msg) {
    const params = msg.params === undefined ? {} : msg.params;
    const flags = {};
    if (HANDOFF_METHODS.has(msg.method)) flags.handOff = true;
    // [SEC-AUZ-022] / spec/bindings/mcp.md 8.2.1: both are steering operations, whatever the
    // thread state, so every call is flagged even when the fake refuses it.
    if (STEERING_METHODS.has(msg.method)) flags.steering = true;
    const allowed = msg.method === 'thread/queue/add' ? QUEUE_ADD_MEMBERS : msg.method === 'turn/start' ? TURN_START_MEMBERS : null;
    if (allowed && isPlainObject(params)) {
      const extra = Object.keys(params).filter((k) => !allowed.has(k));
      if (extra.length) flags.overrideMembers = extra;
    }
    const entry = {
      seq: this.calls.length + 1,
      connection: conn.id,
      transport: conn.transport,
      clientName: conn.clientName,
      kind: msg.id !== undefined ? 'request' : 'notification',
      method: msg.method,
      id: msg.id ?? null,
      params: structuredClone(params),
      flags,
      outcome: msg.id !== undefined ? 'pending' : 'notification',
    };
    this.calls.push(entry);
    return entry;
  }

  // ---- Codex methods -----------------------------------------------------------------

  dispatch(conn, method, params, isRequest, entry) {
    if (method === 'initialize') return this.initialize(conn, params);
    if (method === 'initialized') {
      if (!isRequest) this.notify(conn, this.t.accountUpdated, structuredClone(this.t.accountUpdated.params));
      return undefined;
    }
    if (!isRequest) return undefined; // other client notifications: nothing recorded to react to
    if (!conn.initialized) throw this.recordedError(this.t.s3.notInitialized);
    if (EXPERIMENTAL_METHODS.has(method) && !conn.experimentalApi) {
      entry.flags.experimentalGateRefused = true;
      // Recorded for thread/queue/add, the only method in EXPERIMENTAL_METHODS.
      throw this.recordedError(this.t.s3.experimental);
    }
    switch (method) {
      case 'thread/start':
        return this.threadStart(conn, params);
      case 'thread/resume':
        return this.threadResume(conn, params);
      case 'turn/start':
        return this.turnStart(conn, params, entry);
      case 'thread/queue/add':
        return this.queueAdd(params);
      case 'thread/loaded/list':
        return this.loadedList(params);
      case 'thread/list':
        return this.threadList(params);
      case 'thread/turns/list':
        return this.turnsList(params);
      case 'turn/steer':
        throw notModelled(method, 'no recorded fixture; an OAC adapter must never call it ([MCPB-CDX-004]); the call is logged with flags.steering');
      default:
        throw notModelled(method, 'no recorded fixture');
    }
  }

  initialize(conn, params) {
    if (conn.initialized) throw notModelled('initialize', 'a second initialize on one connection is not recorded');
    const info = params.clientInfo ?? {};
    conn.initialized = true;
    conn.clientName = typeof info.name === 'string' ? info.name : null;
    conn.experimentalApi = params.capabilities?.experimentalApi === true;
    for (const e of this.calls) if (e.connection === conn.id) e.clientName = conn.clientName;
    const result = structuredClone(this.t.initialize);
    result.userAgent = result.userAgent.replace(/\([^)]*\)\s*$/, `(${info.name ?? ''}; ${info.version ?? ''})`);
    return {
      result,
      after: () => this.notify(conn, this.t.remoteControlStatus, structuredClone(this.t.remoteControlStatus.params)),
    };
  }

  newThread({ cwd, ephemeral = false, archived = false, subagent = null, loaded = true, materialized = false } = {}) {
    const nowMs = this.now();
    const id = uuidv7(nowMs);
    const sec = Math.floor(nowMs / 1000);
    const th = {
      id,
      cwd: cwd ?? null,
      ephemeral,
      archived,
      subagent,
      loaded,
      materialized,
      preview: '',
      createdAt: sec,
      updatedAt: sec,
      recencyAt: sec,
      activeTurn: null,
      lastTurnInterrupted: false,
      turns: [],
      queue: [],
      subscribers: new Set(),
    };
    this.threads.set(id, th);
    return th;
  }

  threadObject(th, template) {
    const tpl = template.thread;
    const o = structuredClone(tpl);
    o.id = th.id;
    o.sessionId = th.id;
    o.preview = th.preview;
    o.ephemeral = th.ephemeral;
    o.createdAt = th.createdAt;
    o.updatedAt = th.updatedAt;
    o.recencyAt = th.recencyAt;
    o.status = th.activeTurn ? structuredClone(this.t.statusActive.params.status) : structuredClone(this.t.statusIdle.params.status);
    o.path = typeof tpl.path === 'string' ? tpl.path.split(tpl.id).join(th.id) : tpl.path;
    o.turns = [];
    if (th.cwd !== null) {
      const recorded = tpl.cwd;
      o.cwd = th.cwd;
      o.environments = JSON.parse(JSON.stringify(tpl.environments).split(JSON.stringify(recorded).slice(1, -1)).join(JSON.stringify(th.cwd).slice(1, -1)));
    }
    return o;
  }

  threadStart(conn, params) {
    const extra = Object.keys(params).filter((k) => k !== 'cwd');
    if (extra.length) throw notModelled('thread/start', `only the cwd member is recorded, got ${extra.join(', ')}`);
    if (params.cwd !== undefined && typeof params.cwd !== 'string') throw notModelled('thread/start', 'cwd must be a string');
    const th = this.newThread({ cwd: params.cwd });
    th.subscribers.add(conn);
    const result = structuredClone(this.t.threadStart);
    result.thread = this.threadObject(th, this.t.threadStart);
    if (params.cwd !== undefined) {
      result.cwd = params.cwd;
      result.runtimeWorkspaceRoots = [params.cwd];
    }
    return { result, after: () => this.broadcast(th, this.t.threadStarted, { thread: this.threadObject(th, this.t.threadStart) }) };
  }

  threadResume(conn, params) {
    const extra = Object.keys(params).filter((k) => k !== 'threadId' && k !== 'excludeTurns');
    if (extra.length) throw notModelled('thread/resume', `only threadId and excludeTurns are recorded, got ${extra.join(', ')}`);
    if (params.excludeTurns !== true) throw notModelled('thread/resume', 'only excludeTurns: true is recorded');
    const th = this.threads.get(params.threadId);
    // An unknown id: recorded in the S3 capture (#343). A known thread before its first
    // turn: recorded in D6 attempt 1. Both answer -32600 "no rollout found for thread id <id>".
    if (!th) throw this.recordedError(this.t.s3.resumeUnknown, this.t.s3.resumeUnknownThreadId, params.threadId);
    if (!th.materialized) throw this.recordedError(this.t.resumeFail, this.t.resumeFailThreadId, params.threadId);
    if (th.ephemeral || th.archived || th.subagent) throw notModelled('thread/resume', 'resuming an ephemeral, archived or subagent thread is not recorded');
    const wasLoaded = th.loaded;
    th.loaded = true;
    th.subscribers.add(conn);
    // Loading a thread that was not loaded (S3 capture, `cases-reload`): thread/status/changed
    // idle before the response; after it, thread/goal/cleared, then the head of the queue, if
    // any, is dispatched (the queued add waited for the load).
    if (!wasLoaded) this.broadcast(th, this.t.statusIdle, { threadId: th.id, status: structuredClone(this.t.statusIdle.params.status) });
    const result = structuredClone(this.t.threadResume);
    result.thread = this.threadObject(th, this.t.threadResume);
    for (const k of ['turnsBackwardsCursor', 'itemsBackwardsCursor']) {
      if (typeof result[k] === 'string') result[k] = result[k].split(this.t.threadResume.thread.id).join(th.id);
    }
    return {
      result,
      after: () => {
        this.broadcast(th, this.t.goalCleared, { threadId: th.id });
        if (!wasLoaded) this.wakeIfLoaded(th);
      },
    };
  }

  loadedThread(method, threadId) {
    const th = this.threads.get(threadId);
    if (!th) throw notModelled(method, 'an unknown thread id is not recorded for this method');
    if (!th.loaded) throw notModelled(method, 'a thread that is not loaded is not recorded for this method');
    if (th.ephemeral || th.archived || th.subagent) throw notModelled(method, 'ephemeral, archived and subagent threads are not recorded for this method');
    return th;
  }

  turnStart(conn, params, entry) {
    const th = this.loadedThread('turn/start', params.threadId);
    const input = textInput('turn/start', params.input);
    if (entry.flags.overrideMembers) throw notModelled('turn/start', `setting overrides (${entry.flags.overrideMembers.join(', ')}) are not recorded; the call is logged with flags.overrideMembers`);
    if (th.activeTurn) {
      // Recorded once live (G5 C13 E1, 0.160.0, transcript L58/L66): a turn/start during a
      // running turn returns that turn's id, and its input joins that turn.
      entry.flags.steered = true;
      const turn = th.activeTurn;
      turn.items.push(this.userItem(input, null));
      const result = structuredClone(this.t.steerResp);
      result.turn.id = turn.id;
      return { result };
    }
    const result = structuredClone(this.t.turnStart);
    const turn = this.beginTurn(th, input, null);
    result.turn.id = turn.id;
    return { result, after: () => this.emitTurnStart(th, turn) };
  }

  userItem(input, clientId) {
    const item = structuredClone(this.t.userItemStarted.params.item);
    item.id = uuidv7(this.now());
    item.clientId = clientId;
    const recordedPart = this.t.userItemStarted.params.item.content[0];
    item.content = input.map((p) => ({ ...structuredClone(recordedPart), type: 'text', text: p.text }));
    return item;
  }

  beginTurn(th, input, clientId) {
    const startedMs = this.now();
    const turn = { id: uuidv7(startedMs), startedMs, items: [this.userItem(input, clientId)], status: 'inProgress', completedMs: null };
    th.activeTurn = turn;
    th.lastTurnInterrupted = false;
    th.materialized = true;
    if (!th.preview) th.preview = input.map((p) => p.text).join('\n');
    th.updatedAt = Math.floor(startedMs / 1000);
    return turn;
  }

  turnObject(turn, template, itemsView, items) {
    const o = structuredClone(template);
    o.id = turn.id;
    o.items = items;
    o.itemsView = itemsView;
    o.status = turn.status;
    o.error = null;
    o.startedAt = Math.floor(turn.startedMs / 1000);
    o.completedAt = turn.completedMs === null ? null : Math.floor(turn.completedMs / 1000);
    o.durationMs = turn.completedMs === null ? null : turn.completedMs - turn.startedMs;
    return o;
  }

  emitTurnStart(th, turn) {
    const item = turn.items[0];
    this.broadcast(th, this.t.statusActive, { threadId: th.id, status: structuredClone(this.t.statusActive.params.status) });
    this.broadcast(th, this.t.turnStarted, {
      threadId: th.id,
      turn: this.turnObject({ ...turn, status: 'inProgress', completedMs: null }, this.t.turnStarted.params.turn, this.t.turnStarted.params.turn.itemsView, []),
    });
    const tpl = item.clientId === null ? this.t.userItemStarted : this.t.queuedUserItemStarted;
    const ms = this.now();
    this.broadcast(th, tpl, { ...structuredClone(tpl.params), item, threadId: th.id, turnId: turn.id, startedAtMs: ms });
    const done = structuredClone(this.t.userItemCompleted.params);
    this.broadcast(th, this.t.userItemCompleted, { ...done, item, threadId: th.id, turnId: turn.id, completedAtMs: ms });
  }

  queueAdd(params) {
    const extra = Object.keys(params).filter((k) => !QUEUE_ADD_MEMBERS.has(k));
    // Upstream handler order (thread_queue_processor.rs add(): require_thread, then
    // ensure_direct_input_allowed, then service(), then enqueue).
    if (typeof params.threadId !== 'string' || !UUID_RE.test(params.threadId)) throw notModelled('thread/queue/add', 'the message for a malformed thread id is not recorded');
    const th = this.threads.get(params.threadId);
    // Recorded (S3 capture, #343): the ephemeral, unknown-thread and archived refusals. An
    // unknown id is -32603, an internal error from the thread store, not the -32600
    // "thread not found" that thread_queue_processor.rs maps ThreadNotFound to: the local
    // store reports a missing rollout as an invalid request instead.
    // oacFake/thread/create only makes the recorded combinations: an archived thread is
    // materialized and not loaded, an ephemeral one loaded with no turn (createThread).
    const s3 = this.t.s3;
    if (!th) throw this.recordedError(s3.addUnknown, s3.addUnknownThreadId, params.threadId);
    if (th.archived) throw this.recordedError(s3.addArchived, s3.addArchivedThreadId, th.id);
    if (th.ephemeral) throw this.recordedError(s3.addEphemeral, s3.addEphemeralThreadId, th.id);
    // Upstream refuses some subagent threads (ensure_direct_input_allowed) and accepts
    // others; no fixture records either answer.
    if (th.subagent) throw notModelled('thread/queue/add', 'no fixture records an add to a subagent thread');
    // Every recorded accepted add went to a thread that had already run a turn.
    if (!th.materialized) throw notModelled('thread/queue/add', 'no fixture records an add to a thread before its first turn');
    const input = textInput('thread/queue/add', params.input);
    if (typeof params.clientUserMessageId !== 'string') throw notModelled('thread/queue/add', 'the error for a missing clientUserMessageId is not recorded');
    void extra; // extra members are flagged in the call log and accepted, as recorded (S3 capture)
    const queued = { id: uuidv7(this.now()), input, clientUserMessageId: params.clientUserMessageId };
    th.queue.push(queued);
    const result = structuredClone(this.t.queueAdd);
    const recordedPart = this.t.queueAdd.queuedSubmission.input[0];
    result.queuedSubmission = {
      ...result.queuedSubmission,
      id: queued.id,
      input: input.map((p) => ({ ...structuredClone(recordedPart), type: 'text', text: p.text })),
      clientUserMessageId: queued.clientUserMessageId,
    };
    // Recorded order: D6 conn1's thread/queue/changed (t=00:00:33.171Z) precedes the
    // conn3-queue response (t=00:00:33.174Z). On an idle, loaded thread whose last turn
    // was not interrupted (S3 capture: the idle add on the TUI's thread, and the
    // extra-member add), the head is taken off the queue before the response too: a second
    // thread/queue/changed, then the response, then thread/status/changed active,
    // turn/started and the userMessage carrying clientId = clientUserMessageId.
    this.broadcast(th, this.t.queueChanged, { threadId: th.id });
    const turn = this.mayDispatch(th) ? this.takeHead(th) : null;
    return { result, after: () => turn && this.emitTurnStart(th, turn) };
  }

  // service.rs wake_if_loaded / dispatch_if_idle: the head of the queue starts a turn when
  // the thread is loaded, idle, and its last turn did not end interrupted (all recorded:
  // S3 capture).
  mayDispatch(th) {
    return th.loaded && !th.activeTurn && !th.lastTurnInterrupted && th.queue.length > 0;
  }

  wakeIfLoaded(th) {
    if (this.mayDispatch(th)) this.dispatchHead(th);
  }

  // Take the head off the queue (one item per idle: S3 capture, two adds during a turn) and
  // begin its turn; the caller emits the turn's frames.
  takeHead(th) {
    const head = th.queue.shift();
    this.broadcast(th, this.t.queueChanged, { threadId: th.id });
    return this.beginTurn(th, head.input, head.clientUserMessageId);
  }

  dispatchHead(th) {
    if (!th.queue.length) return;
    this.emitTurnStart(th, this.takeHead(th));
  }

  loadedList(params) {
    if (Object.keys(params).length) throw notModelled('thread/loaded/list', 'only empty params are recorded');
    const loaded = [...this.threads.values()].filter((t) => t.loaded);
    if (loaded.some((t) => t.subagent || t.ephemeral)) throw notModelled('thread/loaded/list', 'no fixture records a loaded subagent or ephemeral thread in the list');
    const r = structuredClone(this.t.loadedList);
    r.data = loaded.map((t) => t.id);
    r.nextCursor = null;
    return { result: r };
  }

  threadList(params) {
    if (params.cursor !== null && params.cursor !== undefined) throw notModelled('thread/list', 'paging by cursor is not modelled');
    if (params.sortKey !== undefined && params.sortKey !== 'created_at') throw notModelled('thread/list', 'only sortKey created_at is recorded');
    const extra = Object.keys(params).filter((k) => !['cursor', 'limit', 'sortKey'].includes(k));
    if (extra.length) throw notModelled('thread/list', `only cursor, limit and sortKey are recorded, got ${extra.join(', ')}`);
    const limit = Number.isInteger(params.limit) && params.limit > 0 ? params.limit : Infinity;
    if ([...this.threads.values()].some((t) => t.subagent || t.ephemeral || t.archived)) throw notModelled('thread/list', 'no fixture records whether a subagent, ephemeral or archived thread is listed');
    // G2 0.160.0 L17-L20: a loaded thread is not listed before its first turn.
    const rows = [...this.threads.values()]
      .filter((t) => t.materialized)
      .sort((a, b) => b.createdAt - a.createdAt)
      .slice(0, limit);
    const r = structuredClone(this.t.threadList);
    delete r._fixtureNote; // added by fixture redaction, not on the wire
    r.data = rows.map((t) => this.threadObject(t, this.t.threadStart));
    r.nextCursor = null;
    r.backwardsCursor = null;
    return { result: r };
  }

  turnsList(params) {
    const th = this.loadedThread('thread/turns/list', params.threadId);
    if (params.sortDirection !== 'desc' || params.itemsView !== 'full') throw notModelled('thread/turns/list', 'only sortDirection desc with itemsView full is recorded');
    const extra = Object.keys(params).filter((k) => !['threadId', 'limit', 'sortDirection', 'itemsView'].includes(k));
    if (extra.length) throw notModelled('thread/turns/list', `unrecorded members ${extra.join(', ')}`);
    // A thread made materialized by oacFake/thread/create stands for one that has run turns
    // the fake never saw, so its list would leave them out. Every recorded list has at least
    // one turn, so an empty one is not recorded either.
    if (th.priorTurnsUnmodelled) throw notModelled('thread/turns/list', 'the thread was made by oacFake/thread/create with earlier turns the fake does not model');
    const limit = Number.isInteger(params.limit) && params.limit > 0 ? params.limit : Infinity;
    const all = [...th.turns, ...(th.activeTurn ? [th.activeTurn] : [])].reverse().slice(0, limit);
    if (all.length === 0) throw notModelled('thread/turns/list', 'no fixture records a thread with no turn');
    const r = structuredClone(this.t.turnsList);
    r.data = all.map((turn) => this.turnObject(turn, this.t.turnsList.data[0], 'full', structuredClone(turn.items)));
    r.nextCursor = null;
    if (typeof r.backwardsCursor === 'string') r.backwardsCursor = null;
    return { result: r };
  }

  // ---- test control (oacFake/*) ------------------------------------------------------

  control(method, p) {
    switch (method) {
      case 'oacFake/turn/complete':
        return { result: this.completeTurn(p) };
      case 'oacFake/thread/create':
        return { result: { threadId: this.createThread(p).id } };
      case 'oacFake/thread/setArchived':
        return { result: this.setArchived(p) };
      case 'oacFake/calls':
        return { result: this.callReport(p) };
      case 'oacFake/thread/state': {
        const th = this.threads.get(p.threadId);
        if (!th) throw new RpcError(-32602, `unknown thread ${p.threadId}`);
        return {
          result: {
            threadId: th.id,
            loaded: th.loaded,
            materialized: th.materialized,
            activeTurnId: th.activeTurn?.id ?? null,
            lastTurnInterrupted: th.lastTurnInterrupted,
            queue: structuredClone(th.queue),
            turns: th.turns.map((t) => ({ id: t.id, status: t.status, items: structuredClone(t.items) })),
            subscribers: th.subscribers.size,
          },
        };
      }
      case 'oacFake/templates':
        return { result: { sources: this.templateSources } };
      default:
        throw new RpcError(-32601, `unknown fake control method ${method}`);
    }
  }

  // oacFake/thread/create. Only states a fixture records, or (subagent) states every Codex
  // method answers NOT_MODELLED for; any other combination is refused:
  //   - archived: materialized and not loaded, as after `thread/archive` (S3 L1023-L1030);
  //   - ephemeral: loaded, before any turn (`materialized` defaults to false), as made by
  //     `thread/start` with `ephemeral` (S3 L111-L114); an unloaded or turned one is not
  //     recorded;
  //   - not loaded: only after a turn (S3 L956-L1019); a thread never turned and not loaded
  //     is not recorded;
  //   - subagent: neither archived nor ephemeral;
  //   - materialized (the default): a thread that has already run turns; their content is
  //     not modelled, so `thread/turns/list` on it answers NOT_MODELLED.
  createThread(p) {
    const subagent = p.subagent ?? null;
    if (subagent !== null && !SUBAGENT_KINDS.includes(subagent)) throw new RpcError(-32602, `subagent must be one of ${SUBAGENT_KINDS.join(', ')}`);
    const ephemeral = p.ephemeral === true;
    const archived = p.archived === true;
    const loaded = p.loaded !== false;
    const materialized = p.materialized === undefined ? !ephemeral : p.materialized !== false;
    const refuse = (why) => new RpcError(-32602, `oacFake/thread/create: ${why}`);
    if (subagent !== null && (ephemeral || archived)) throw refuse('no fixture records an ephemeral or archived subagent thread');
    if (archived && (loaded || !materialized || ephemeral)) throw refuse('an archived thread is recorded only materialized and not loaded (S3 L1023-L1030): pass loaded: false');
    if (ephemeral && (!loaded || materialized)) throw refuse('an ephemeral thread is recorded only loaded and before any turn (S3 L111-L119)');
    if (!loaded && !materialized) throw refuse('a thread that is not loaded is recorded only after a turn (S3 L956-L1019)');
    const th = this.newThread({ cwd: p.cwd, ephemeral, archived, subagent, loaded, materialized });
    // Materialized means it has run a turn (G2 L17-L20); the fake has no record of that turn.
    th.priorTurnsUnmodelled = materialized;
    return th;
  }

  // Test set-up, not a Codex behaviour: put an idle top-level thread into the state the S3
  // capture recorded after `thread/archive` (archived and not loaded, `cases-reload`
  // L1023-L1030), where an add gets the recorded archived refusal (L1030-L1032); or put it
  // back as it was (loaded, not archived). It sends no frame: neither `thread/archive`'s
  // notifications nor anything for `thread/unarchive` (not recorded) are modelled. The
  // restore leaves a recorded state (loaded and idle, S3 L981-L1018) by a transition no
  // fixture records (no `thread/unarchive` capture). Only IFC-ADP-056's detection of a
  // re-sent refused add uses it; SEC-AUZ-027 and IFC-ADP-057 catch that case without it.
  setArchived({ threadId, archived }) {
    const th = this.threads.get(threadId);
    if (!th) throw new RpcError(-32602, `unknown thread ${threadId}`);
    if (typeof archived !== 'boolean') throw new RpcError(-32602, 'archived must be true or false');
    if (th.ephemeral || th.subagent || !th.materialized) throw new RpcError(-32602, 'only a materialized top-level thread is archived in the S3 capture');
    if (th.activeTurn || th.queue.length) throw new RpcError(-32602, 'archiving a thread with a running turn or a queue is not recorded');
    if (archived === th.archived) throw new RpcError(-32602, `thread ${threadId} is already ${archived ? 'archived' : 'not archived'}`);
    if (archived && !th.loaded) throw new RpcError(-32602, 'the S3 capture archived a loaded thread');
    th.archived = archived;
    th.loaded = !archived;
    return { threadId: th.id, archived: th.archived, loaded: th.loaded };
  }

  // Finish the running turn. status "completed" (recorded, D6 conn1) or "interrupted"
  // (recorded, S3 capture: the frames of a turn/interrupt, and nothing dispatched after it).
  completeTurn({ threadId, status = 'completed', agentText = null }) {
    const th = this.threads.get(threadId);
    if (!th) throw new RpcError(-32602, `unknown thread ${threadId}`);
    if (!th.activeTurn) throw new RpcError(-32602, `thread ${threadId} has no running turn`);
    if (status !== 'completed' && status !== 'interrupted') throw new RpcError(-32602, 'status must be completed or interrupted');
    const turn = th.activeTurn;
    const summary = [];
    if (agentText !== null) {
      const item = structuredClone(this.t.agentItemStarted.params.item);
      item.id = agentItemId();
      item.text = '';
      const s = this.now();
      this.broadcast(th, this.t.agentItemStarted, { ...structuredClone(this.t.agentItemStarted.params), item: structuredClone(item), threadId: th.id, turnId: turn.id, startedAtMs: s });
      this.broadcast(th, this.t.agentDelta, { threadId: th.id, turnId: turn.id, itemId: item.id, delta: String(agentText) });
      item.text = String(agentText);
      // An interrupted turn's agent message never completes (S3 capture L899-L907: its
      // item/started and delta, then idle and turn/completed, and no item/completed for it).
      if (status !== 'interrupted') {
        this.broadcast(th, this.t.agentItemCompleted, { ...structuredClone(this.t.agentItemCompleted.params), item: structuredClone(item), threadId: th.id, turnId: turn.id, completedAtMs: this.now() });
        turn.items.push(item);
        summary.push(item);
      }
    }
    turn.status = status;
    turn.completedMs = this.now();
    th.activeTurn = null;
    th.turns.push(turn);
    th.lastTurnInterrupted = status === 'interrupted';
    th.updatedAt = Math.floor(turn.completedMs / 1000);
    this.broadcast(th, this.t.statusIdle, { threadId: th.id, status: structuredClone(this.t.statusIdle.params.status) });
    // An interrupted turn's turn/completed is recorded (S3 capture): no items, itemsView
    // "notLoaded", even when agent text had streamed before the interrupt.
    const tpl = status === 'interrupted' ? this.t.s3.interruptedTurnCompleted.params.turn : this.t.turnCompleted.params.turn;
    this.broadcast(th, this.t.turnCompleted, {
      threadId: th.id,
      turn: this.turnObject(turn, tpl, tpl.itemsView, status === 'interrupted' ? [] : structuredClone(summary)),
    });
    // service.rs on_thread_idle: an idle caused by an interrupt dispatches nothing.
    if (status === 'completed') this.dispatchHead(th);
    return { threadId: th.id, turnId: turn.id, status, dispatched: th.activeTurn?.id ?? null };
  }

  callReport({ clientName } = {}) {
    const mine = (list) => (clientName === undefined ? list : list.filter((c) => c.clientName === clientName));
    const calls = mine(this.calls);
    return {
      calls: structuredClone(calls),
      steering: calls.filter((c) => c.flags.steering).map((c) => c.seq),
      overrideMembers: calls.filter((c) => c.flags.overrideMembers).map((c) => c.seq),
      handOffs: calls.filter((c) => c.flags.handOff).map((c) => c.seq),
      malformed: calls.filter((c) => c.flags.malformed || c.flags.malformedParams).map((c) => c.seq),
      // oacFake/* calls, numbered separately: an adapter should never make one.
      control: structuredClone(mine(this.controlCalls)),
    };
  }
}

export { STEERING_METHODS, EXPERIMENTAL_METHODS };
