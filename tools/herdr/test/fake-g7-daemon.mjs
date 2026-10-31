// TEST DOUBLE of the Codex app-server daemon, used only by the herdr driver's self-test
// (tools/herdr/test/g7-tests.mjs) to run the REAL quarantined G7 client
// (docs/planning/gates/fixtures/g7-codex-recheck/client.mjs.throwaway-quarantined) end to end
// with no Codex installed. It is not Codex and proves nothing about Codex: its turns, its
// queue and its unloading are this file's inventions, shaped after the S3 capture
// (docs/planning/gates/fixtures/s3-codex-capture/) so that the client's cases and the
// scenario's judgments have something to run against. What it does with a resume override,
// and whether it implements queue management, are options, so each judgment can be reached.
//
// Loopback only (the WebSocket listener of tests/fakes/codex-app-server/lib/ws.mjs).
// Together with test/fake-g7-codex.mjs, which stands in for the `codex` executable
// (`codex app-server proxy` is a byte relay to this listener).

import { listen } from '../../../tests/fakes/codex-app-server/lib/ws.mjs';

// options:
//   overrideLoaded  how a resume override is treated on a LOADED thread. 'real' (default) follows the
//                   subscriber rule read at rust-v0.166.0 (thread_processor.rs L4697-L4748): a thread
//                   with a subscriber ignores it (the thread's own value is reported); an idle thread with
//                   none is shut down and resumed cold with it (stored, still loaded afterwards).
//                   'applied' (always stored, thread/settings/updated sent), 'echo' (the answer reports
//                   the override, nothing is stored) and 'ignored' (never applied) are the other
//                   behaviours a judgment can meet. A thread that is not loaded always takes it.
//   queueMgmt       thread/queue/list|update|delete|reorder implemented (true) or "method not found"
//   approvalAtThread  a thread number (1 = the first thread started) whose first turn sends a
//                   server->client approval request and then waits for ever
//   unloadMs, turnMs, busyMs  timings
//   foreign         send notifications about a thread the client did not start (privacy check)
//   rejectEffort    turn/start with an `effort` member is refused (-32602), to exercise the client's fallback
//   startedBeforeAnswer  send thread/started for a new thread BEFORE the thread/start answer (the client
//                   holds a frame about a thread it does not know yet until the answer names it)
export async function startFakeG7Daemon({ overrideLoaded = 'real', queueMgmt = true, approvalAtThread = null, unloadMs = 250, turnMs = 40, busyMs = 700, foreign = true, startedBeforeAnswer = true, rejectEffort = false, version = '9.9.9' } = {}) {
  const threads = new Map();
  const conns = new Set();
  const received = []; // every message a client sent: { conn, msg }
  const overrideLog = []; // each resume that carried an override: the thread's state when it arrived
  let joinedTurns = false; // a turn/start arrived while the thread had a turn running (a steer)
  let n = 0;
  let threadCount = 0;
  let connSeq = 0;
  const nextId = (p) => `${p}-${++n}`;

  const send = (c, msg) => c.ch.send(JSON.stringify(msg));
  const notify = (th, method, params) => {
    for (const c of th.subs) send(c, { method, params });
  };
  const status = (th, type) => notify(th, 'thread/status/changed', { threadId: th.id, status: { type } });
  const settingsOf = (th) => ({ approvalPolicy: th.settings.approvalPolicy, sandbox: { type: 'workspaceWrite' }, model: 'fake-model', reasoningEffort: null });

  function startTurn(th, input, clientId = null, { busy = false, id = null } = {}) {
    const turn = { id: id ?? nextId('turn'), timers: [] };
    th.active = turn;
    th.turns += 1;
    status(th, 'active');
    notify(th, 'turn/started', { threadId: th.id, turn: { id: turn.id, status: 'inProgress' } });
    notify(th, 'item/started', { threadId: th.id, turnId: turn.id, item: { type: 'userMessage', id: nextId('item'), clientId, content: input } });
    if (approvalAtThread === th.number && !th.approvalSent) {
      th.approvalSent = true;
      turn.timers.push(setTimeout(() => {
        const [c] = [...th.subs];
        if (c) send(c, { id: 9001, method: 'item/commandExecution/requestApproval', params: { threadId: th.id, turnId: turn.id } });
      }, 20));
      return turn;
    }
    turn.timers.push(setTimeout(() => notify(th, 'item/agentMessage/delta', { threadId: th.id, turnId: turn.id, itemId: 'agent', delta: 'x' }), 20));
    turn.timers.push(setTimeout(() => completeTurn(th, 'completed'), busy ? busyMs : turnMs));
    return turn;
  }
  function completeTurn(th, statusName) {
    const turn = th.active;
    if (!turn) return;
    turn.timers.forEach(clearTimeout);
    th.active = null;
    th.lastInterrupted = statusName === 'interrupted';
    status(th, 'idle');
    notify(th, 'turn/completed', { threadId: th.id, turn: { id: turn.id, status: statusName, items: [] } });
    if (statusName === 'completed') setTimeout(() => dispatch(th), 30);
  }
  function dispatch(th) {
    if (!th.loaded || th.active || th.lastInterrupted || th.archived || !th.queue.length) return;
    const item = th.queue.shift();
    notify(th, 'thread/queue/changed', { threadId: th.id });
    startTurn(th, item.input, item.clientUserMessageId);
  }

  const err = (c, id, code, message) => send(c, { id, error: { code, message } });
  const ok = (c, id, result) => send(c, { id, result });
  const own = (c, id, p) => {
    const th = threads.get(p?.threadId);
    if (!th) err(c, id, -32600, `thread not found: ${p?.threadId}`);
    return th;
  };
  const resumeResult = (th, shown) => ({ thread: { id: th.id, status: { type: th.active ? 'active' : 'idle' } }, ...settingsOf(th), ...(shown ? { approvalPolicy: shown } : {}) });

  function handle(c, msg) {
    received.push({ conn: c.name, connId: c.id, msg });
    const { id, method, params } = msg;
    if (method === undefined) return; // a response to a request of ours: never expected
    if (method === 'initialized') {
      if (foreign) {
        send(c, { method: 'thread/started', params: { thread: { id: 'foreign-thread-1', preview: 'SECRET-FOREIGN-PREVIEW', cwd: '/secret/foreign/cwd' } } });
        send(c, { method: 'thread/status/changed', params: { threadId: 'foreign-thread-1', status: { type: 'idle' } } });
      }
      return;
    }
    if (method === 'initialize') {
      c.name = params?.clientInfo?.name ?? 'unknown';
      return ok(c, id, { userAgent: `codex-tui/${version} (Fake OS; x86_64) fake (${c.name}; 0.0.1)` });
    }
    if (method === 'thread/loaded/list') return ok(c, id, { data: [...threads.values()].filter((t) => t.loaded).map((t) => t.id).concat(foreign ? ['foreign-thread-1'] : []), nextCursor: null });
    if (method === 'thread/start') {
      const th = { id: `thr-${++threadCount}-${n++}`, number: threadCount, loaded: true, subs: new Set([c]), turns: 0, queue: [], archived: false, active: null, lastInterrupted: false, settings: { approvalPolicy: 'on-request' } };
      threads.set(th.id, th);
      if (startedBeforeAnswer) notify(th, 'thread/started', { thread: { id: th.id } });
      ok(c, id, { thread: { id: th.id, status: { type: 'idle' }, ephemeral: false, cwd: params?.cwd }, ...settingsOf(th) });
      if (!startedBeforeAnswer) notify(th, 'thread/started', { thread: { id: th.id } });
      return;
    }
    const th = own(c, id, params);
    if (!th) return;
    if (method === 'turn/start') {
      if (rejectEffort && params.effort !== undefined) return err(c, id, -32602, 'unknown member effort');
      if (th.active) {
        joinedTurns = true;
        return ok(c, id, { turn: { id: th.active.id, status: 'inProgress' } }); // joins the running turn (a steer): the client must never get here
      }
      const input = params.input;
      const busy = /busy turn/.test(JSON.stringify(input));
      const turnId = nextId('turn');
      ok(c, id, { turn: { id: turnId, status: 'inProgress' } });
      startTurn(th, input, null, { busy, id: turnId });
      return;
    }
    if (method === 'turn/interrupt') {
      ok(c, id, {});
      return completeTurn(th, 'interrupted');
    }
    if (method === 'thread/queue/add') {
      if (th.archived) return err(c, id, -32600, `session ${th.id} is archived`);
      const item = { id: nextId('q'), clientUserMessageId: params.clientUserMessageId, input: params.input };
      if (th.loaded && !th.active && !th.lastInterrupted) {
        ok(c, id, { queuedSubmission: item });
        return startTurn(th, item.input, item.clientUserMessageId);
      }
      th.queue.push(item);
      notify(th, 'thread/queue/changed', { threadId: th.id });
      return ok(c, id, { queuedSubmission: item });
    }
    if (/^thread\/queue\/(list|update|delete|reorder)$/.test(method)) {
      if (!queueMgmt) return err(c, id, -32601, `method not found: ${method}`);
      if (method === 'thread/queue/list') return ok(c, id, { data: th.queue.map((q) => ({ ...q })), nextCursor: null });
      if (method === 'thread/queue/update') {
        const q = th.queue.find((x) => x.id === params.queuedSubmissionId);
        if (!q) return err(c, id, -32600, 'no such queued submission');
        q.input = params.input;
        notify(th, 'thread/queue/changed', { threadId: th.id });
        return ok(c, id, { queuedSubmission: { ...q } });
      }
      if (method === 'thread/queue/delete') {
        const i = th.queue.findIndex((x) => x.id === params.queuedSubmissionId);
        if (i >= 0) th.queue.splice(i, 1);
        notify(th, 'thread/queue/changed', { threadId: th.id });
        return ok(c, id, { deleted: i >= 0 });
      }
      const order = params.queuedSubmissionIds.map((qid) => th.queue.find((x) => x.id === qid)).filter(Boolean);
      if (order.length !== th.queue.length) return err(c, id, -32600, 'the ids are not the queue');
      th.queue = order;
      notify(th, 'thread/queue/changed', { threadId: th.id });
      return ok(c, id, {});
    }
    if (method === 'thread/unsubscribe') {
      th.subs.delete(c);
      ok(c, id, { status: 'unsubscribed' });
      if (!th.subs.size) {
        setTimeout(() => {
          if (th.subs.size || th.active) return;
          th.loaded = false;
          for (const cc of conns) send(cc, { method: 'thread/closed', params: { threadId: th.id } }); // every connection, as the real daemon does
        }, unloadMs);
      }
      return;
    }
    if (method === 'thread/resume') {
      if (!th.turns) return err(c, id, -32600, `no rollout found for thread id ${th.id}`);
      const wasLoaded = th.loaded;
      const hadSubscribers = th.subs.size > 0; // before this connection subscribes, as the real daemon checks
      th.subs.add(c);
      th.loaded = true;
      const override = params.approvalPolicy;
      if (override !== undefined) overrideLog.push({ loaded: wasLoaded, hadSubscribers, idle: !th.active });
      let shown = null;
      if (override !== undefined) {
        const cold = overrideLoaded === 'real' && !hadSubscribers && !th.active;
        if (!wasLoaded || overrideLoaded === 'applied' || cold) {
          th.settings.approvalPolicy = override;
          if (wasLoaded) notify(th, 'thread/settings/updated', { threadId: th.id, threadSettings: { approvalPolicy: override } });
        } else if (overrideLoaded === 'echo') shown = override;
      }
      ok(c, id, resumeResult(th, shown));
      if (!wasLoaded) setTimeout(() => dispatch(th), 20);
      return;
    }
    if (method === 'thread/archive') {
      th.archived = true;
      th.loaded = false;
      return ok(c, id, {});
    }
    return err(c, id, -32601, `method not found: ${method}`);
  }

  const server = await listen('ws://127.0.0.1:0', (ch) => {
    const c = { ch, name: 'unknown', id: ++connSeq };
    conns.add(c);
    ch.on('message', (text) => {
      let msg;
      try {
        msg = JSON.parse(text);
      } catch {
        return;
      }
      try {
        handle(c, msg);
      } catch (e) {
        process.stderr.write(`fake-g7-daemon: ${e.stack}\n`);
      }
    });
    ch.on('close', () => {
      conns.delete(c);
      for (const th of threads.values()) th.subs.delete(c);
    });
  });
  return {
    url: server.url,
    port: Number(new URL(server.url).port),
    received,
    overrideLog,
    get joinedTurns() {
      return joinedTurns;
    },
    threads,
    close: async () => {
      for (const th of threads.values()) th.active?.timers.forEach(clearTimeout);
      await server.close();
    },
  };
}
