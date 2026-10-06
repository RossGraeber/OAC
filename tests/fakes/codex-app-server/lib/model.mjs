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
//   - A behaviour with no recorded fixture is either cited to upstream source in README
//     ("source-only", runtime UNVERIFIED) or answered with the fake's own NOT_MODELLED error.
//     The fake never invents a Codex error message or a Codex frame shape.
//   - Turns never progress on a timer. A turn runs until a test calls
//     `oacFake/turn/complete` (no sleeps, no polling, deterministic order).

import { randomBytes } from 'node:crypto';
import { loadTemplates } from './fixtures.mjs';

// JSON-RPC codes in the implementation-defined server-error range, used only by the fake.
export const NOT_MODELLED = -32099;
export const FAKE_INTERNAL = -32098;
const isPlainObject = (v) => v !== null && typeof v === 'object' && !Array.isArray(v);
export const CONTROL_PREFIX = 'oacFake/';

// From upstream source (README "Source-only behaviours"); code -32600 is
// INVALID_REQUEST_ERROR_CODE in app-server/src/error_code.rs.
const INVALID_REQUEST = -32600;
const EXPERIMENTAL_METHODS = new Set(['thread/queue/add']);
const QUEUE_ADD_MEMBERS = new Set(['threadId', 'input', 'clientUserMessageId']);
const TURN_START_MEMBERS = new Set(['threadId', 'input']);
// Methods that hand input to a thread: every call is flagged in the call log.
const HANDOFF_METHODS = new Set(['thread/queue/add', 'turn/start', 'turn/steer']);
// Methods [SEC-AUZ-022] / spec/bindings/mcp.md 8.2.1 classify as steering operations.
const STEERING_METHODS = new Set(['turn/steer', 'turn/start']);

export const SUBAGENT_KINDS = ['multi-agent-v2', 'thread-spawn'];
const MSG = {
  notInitialized: 'Not initialized',
  experimental: (m) => `${m} requires experimentalApi capability`,
  ephemeral: (id) => `ephemeral thread does not support queued submissions: ${id}`,
  notFound: (id) => `thread not found: ${id}`,
  archived: (id) => `session ${id} is archived. Run \`codex unarchive ${id}\` to unarchive it first.`,
  subagentV2: 'direct app-server input is not allowed for multi-agent v2 sub-agents',
  subagentUnloaded: 'direct app-server input is not allowed for unloaded spawned sub-agents',
  noQueue: 'user message queue is unavailable',
};

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
    this.queueServiceAvailable = true;
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
    if (!conn.initialized) throw new RpcError(INVALID_REQUEST, MSG.notInitialized);
    if (EXPERIMENTAL_METHODS.has(method) && !conn.experimentalApi) {
      entry.flags.experimentalGateRefused = true;
      throw new RpcError(INVALID_REQUEST, MSG.experimental(method));
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
    // Recorded for a thread before its first turn (D6 attempt 1). For an unknown id the
    // same message is source-only: thread-store read_thread.rs L97-L102 returns it whenever
    // no rollout resolves for the id, and thread_processor.rs L3194-L3195 maps
    // ThreadNotFound to it (README "Source-only behaviours").
    if (!th || !th.materialized) {
      const e = this.t.resumeFail;
      throw new RpcError(e.code, e.message.split(this.t.resumeFailThreadId).join(String(params.threadId)));
    }
    if (th.ephemeral || th.archived || th.subagent) throw notModelled('thread/resume', 'resuming an ephemeral, archived or subagent thread is not recorded');
    th.loaded = true;
    th.subscribers.add(conn);
    const result = structuredClone(this.t.threadResume);
    result.thread = this.threadObject(th, this.t.threadResume);
    for (const k of ['turnsBackwardsCursor', 'itemsBackwardsCursor']) {
      if (typeof result[k] === 'string') result[k] = result[k].split(this.t.threadResume.thread.id).join(th.id);
    }
    return { result, after: () => this.broadcast(th, this.t.goalCleared, { threadId: th.id }) };
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
    if (th && th.loaded) {
      if (th.ephemeral) throw new RpcError(INVALID_REQUEST, MSG.ephemeral(th.id));
    } else {
      if (!th) throw new RpcError(INVALID_REQUEST, MSG.notFound(params.threadId));
      if (th.archived) throw new RpcError(INVALID_REQUEST, MSG.archived(th.id));
    }
    if (th.loaded && th.subagent === 'multi-agent-v2') throw new RpcError(INVALID_REQUEST, MSG.subagentV2);
    if (!th.loaded && th.subagent === 'thread-spawn') throw new RpcError(INVALID_REQUEST, MSG.subagentUnloaded);
    if (!this.queueServiceAvailable) throw new RpcError(INVALID_REQUEST, MSG.noQueue);
    const input = textInput('thread/queue/add', params.input);
    if (typeof params.clientUserMessageId !== 'string') throw notModelled('thread/queue/add', 'the error for a missing clientUserMessageId is not recorded');
    void extra; // extra members are flagged in the call log; upstream handling is UNVERIFIED (README)
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
    // conn3-queue response (t=00:00:33.174Z). A dispatch on an idle thread follows the
    // response (enqueue emits the change, then wake_if_loaded runs; README).
    this.broadcast(th, this.t.queueChanged, { threadId: th.id });
    return { result, after: () => this.wakeIfLoaded(th) };
  }

  // service.rs wake_if_loaded / dispatch_if_idle: start the head of the queue if the thread
  // is loaded, idle, and its last turn did not end interrupted.
  wakeIfLoaded(th) {
    if (!th.loaded || th.activeTurn || th.lastTurnInterrupted) return;
    this.dispatchHead(th);
  }

  dispatchHead(th) {
    const head = th.queue.shift();
    if (!head) return;
    this.broadcast(th, this.t.queueChanged, { threadId: th.id });
    const turn = this.beginTurn(th, head.input, head.clientUserMessageId);
    this.emitTurnStart(th, turn);
  }

  loadedList(params) {
    if (Object.keys(params).length) throw notModelled('thread/loaded/list', 'only empty params are recorded');
    const r = structuredClone(this.t.loadedList);
    r.data = [...this.threads.values()].filter((t) => t.loaded).map((t) => t.id);
    r.nextCursor = null;
    return { result: r };
  }

  threadList(params) {
    if (params.cursor !== null && params.cursor !== undefined) throw notModelled('thread/list', 'paging by cursor is not modelled');
    if (params.sortKey !== undefined && params.sortKey !== 'created_at') throw notModelled('thread/list', 'only sortKey created_at is recorded');
    const extra = Object.keys(params).filter((k) => !['cursor', 'limit', 'sortKey'].includes(k));
    if (extra.length) throw notModelled('thread/list', `only cursor, limit and sortKey are recorded, got ${extra.join(', ')}`);
    const limit = Number.isInteger(params.limit) && params.limit > 0 ? params.limit : Infinity;
    // G2 0.160.0 L17-L20: a loaded thread is not listed before its first turn.
    const rows = [...this.threads.values()]
      .filter((t) => t.materialized && !t.ephemeral && !t.archived && !t.subagent)
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
    const limit = Number.isInteger(params.limit) && params.limit > 0 ? params.limit : Infinity;
    const all = [...th.turns, ...(th.activeTurn ? [th.activeTurn] : [])].reverse().slice(0, limit);
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
      case 'oacFake/thread/create': {
        const subagent = p.subagent ?? null;
        if (subagent !== null && !SUBAGENT_KINDS.includes(subagent)) throw new RpcError(-32602, `subagent must be one of ${SUBAGENT_KINDS.join(', ')}`);
        const th = this.newThread({
          cwd: p.cwd,
          ephemeral: p.ephemeral === true,
          archived: p.archived === true,
          subagent,
          loaded: p.loaded !== false,
          materialized: p.materialized !== false,
        });
        return { result: { threadId: th.id } };
      }
      case 'oacFake/queue/setAvailable':
        this.queueServiceAvailable = p.available !== false;
        return { result: { available: this.queueServiceAvailable } };
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

  // Finish the running turn. status "completed" (recorded, D6 conn1) or "interrupted"
  // (source-only: TurnStatus::Interrupted, service.rs on_thread_idle skip).
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
      this.broadcast(th, this.t.agentItemCompleted, { ...structuredClone(this.t.agentItemCompleted.params), item: structuredClone(item), threadId: th.id, turnId: turn.id, completedAtMs: this.now() });
      turn.items.push(item);
      summary.push(item);
    }
    turn.status = status;
    turn.completedMs = this.now();
    th.activeTurn = null;
    th.turns.push(turn);
    th.lastTurnInterrupted = status === 'interrupted';
    th.updatedAt = Math.floor(turn.completedMs / 1000);
    this.broadcast(th, this.t.statusIdle, { threadId: th.id, status: structuredClone(this.t.statusIdle.params.status) });
    this.broadcast(th, this.t.turnCompleted, {
      threadId: th.id,
      turn: this.turnObject(turn, this.t.turnCompleted.params.turn, this.t.turnCompleted.params.turn.itemsView, structuredClone(summary)),
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
