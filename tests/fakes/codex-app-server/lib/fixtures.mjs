// SPDX-License-Identifier: Apache-2.0
//
// Load the recorded Stage 1 Codex fixtures in place, from docs/planning/gates/fixtures/,
// and extract the frame templates the fake replays (#58, F9). Nothing here is
// hand-written wire shape: every template is a frame read from a committed fixture file,
// located by direction, method and (for responses) request id. If a fixture stops
// containing a frame the fake needs, loading fails loudly instead of the fake guessing.
//
// Which file backs which behaviour is README.md "Provenance".

import { readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..', '..', '..');
export const FIXTURE_DIR = 'docs/planning/gates/fixtures';

// Every fixture file the fake reads. Keys are used in provenance records.
export const FIXTURE_FILES = {
  d6Conn1: 'd6-codex-protocol/transcript-conn1-2026-09-28.jsonl',
  d6Conn2: 'd6-codex-protocol/transcript-conn2-2026-09-28.jsonl',
  d6Turn1: 'd6-codex-protocol/transcript-conn3-turn1-2026-09-28.jsonl',
  d6Turn2: 'd6-codex-protocol/transcript-conn3-turn2-2026-09-28.jsonl',
  d6Queue: 'd6-codex-protocol/transcript-conn3-queue-2026-09-28.jsonl',
  d6ResumeFail: 'd6-codex-protocol/transcript-conn2-attempt1-failed-resume-try1-2026-09-27.jsonl',
  g2Lists: 'g2-codex-inject/transcript-2026-10-06-0.160.0-herdr.jsonl',
  g5Steer: 'g5-provenance/transcript-codex-2026-10-02-0.160.0-herdr.jsonl',
};

export const fixturePath = (key) => `${FIXTURE_DIR}/${FIXTURE_FILES[key]}`;

// Parse a transcript into records { line, t, mode, direction, payload }, protocol frames only.
export function readTranscript(key, root = REPO_ROOT) {
  const text = readFileSync(join(root, fixturePath(key)), 'utf8');
  const out = [];
  text.split(/\r?\n/).forEach((l, i) => {
    if (!l.trim()) return;
    const o = JSON.parse(l);
    if (!o.payload || typeof o.payload !== 'object' || !o.direction) return;
    out.push({ line: i + 1, t: o.t, mode: o.mode ?? null, direction: o.direction, payload: o.payload });
  });
  return out;
}

const C2D = 'client->daemon';
const D2C = 'daemon->client';
const clone = (v) => structuredClone(v);

class Templates {
  constructor(root) {
    this.root = root;
    this.cache = new Map();
    this.used = new Map(); // template name -> { file, line }
  }
  frames(key) {
    if (!this.cache.has(key)) this.cache.set(key, readTranscript(key, this.root));
    return this.cache.get(key);
  }
  // The first frame matching `pred`, recorded under `name`.
  pick(name, key, pred) {
    const f = this.frames(key).find(pred);
    if (!f) throw new Error(`fixture ${fixturePath(key)} has no frame for template "${name}"`);
    this.used.set(name, { file: fixturePath(key), line: f.line });
    return f;
  }
  notification(name, key, method, pred = () => true) {
    return this.pick(name, key, (f) => f.direction === D2C && f.payload.method === method && pred(f.payload)).payload;
  }
  // The response to the first `method` request (same id, same `mode` when the file has one).
  response(name, key, method, pred = () => true) {
    const frames = this.frames(key);
    const req = frames.find((f) => f.direction === C2D && f.payload.method === method && pred(f.payload));
    if (!req) throw new Error(`fixture ${fixturePath(key)} has no ${method} request for template "${name}"`);
    return this.pick(
      name,
      key,
      (f) =>
        f.line > req.line &&
        f.direction === D2C &&
        f.payload.method === undefined &&
        f.payload.id === req.payload.id &&
        f.mode === req.mode,
    ).payload;
  }
}

// Build the template set. Throws if a needed frame is missing from its fixture.
export function loadTemplates(root = REPO_ROOT) {
  const T = new Templates(root);
  const isItem = (type) => (p) => p.params?.item?.type === type;
  const status = (type) => (p) => p.params?.status?.type === type;

  const initialize = T.response('initialize', 'd6Conn1', 'initialize').result;
  const threadStart = T.response('thread/start', 'd6Conn1', 'thread/start').result;
  const threadResume = T.response('thread/resume', 'd6Conn2', 'thread/resume').result;
  const resumeFail = T.response('thread/resume no rollout', 'd6ResumeFail', 'thread/resume').error;
  const resumeFailReq = T.frames('d6ResumeFail').find((f) => f.payload.method === 'thread/resume').payload;
  const turnStart = T.response('turn/start', 'd6Turn1', 'turn/start').result;
  const queueAdd = T.response('thread/queue/add', 'd6Queue', 'thread/queue/add').result;
  const loadedList = T.response('thread/loaded/list', 'g2Lists', 'thread/loaded/list').result;
  const threadList = T.response('thread/list', 'g2Lists', 'thread/list').result;
  const turnsList = T.response('thread/turns/list', 'g2Lists', 'thread/turns/list').result;
  // The steering turn/start of the G5 C13 E1 run (G5 transcript L58): its response names
  // the marker turn that was still in progress (spec/bindings/mcp.md 8.2.1 "Observed live once").
  const steerReq = T.frames('g5Steer').find(
    (f) => f.direction === C2D && f.payload.method === 'turn/start' && /g5-0-x2-1\b/.test(JSON.stringify(f.payload.params)),
  );
  if (!steerReq) throw new Error('G5 fixture has no steering turn/start');
  const steerResp = T.pick('turn/start steered', 'g5Steer', (f) => f.line > steerReq.line && f.direction === D2C && f.payload.id === steerReq.payload.id && f.payload.result).payload.result;

  const t = {
    initialize,
    remoteControlStatus: T.notification('remoteControl/status/changed', 'd6Conn1', 'remoteControl/status/changed'),
    accountUpdated: T.notification('account/updated', 'd6Conn1', 'account/updated'),
    threadStart,
    threadStarted: T.notification('thread/started', 'd6Conn1', 'thread/started'),
    threadResume,
    resumeFail,
    resumeFailThreadId: resumeFailReq.params.threadId,
    goalCleared: T.notification('thread/goal/cleared', 'd6Conn2', 'thread/goal/cleared'),
    statusActive: T.notification('thread/status/changed active', 'd6Conn1', 'thread/status/changed', status('active')),
    statusIdle: T.notification('thread/status/changed idle', 'd6Conn1', 'thread/status/changed', status('idle')),
    turnStart,
    turnStarted: T.notification('turn/started', 'd6Conn1', 'turn/started'),
    userItemStarted: T.notification('item/started userMessage', 'd6Conn1', 'item/started', isItem('userMessage')),
    userItemCompleted: T.notification('item/completed userMessage', 'd6Conn1', 'item/completed', isItem('userMessage')),
    queuedUserItemStarted: T.notification('item/started userMessage (queued)', 'd6Conn1', 'item/started', (p) => isItem('userMessage')(p) && p.params.item.clientId !== null),
    agentItemStarted: T.notification('item/started agentMessage', 'd6Conn1', 'item/started', isItem('agentMessage')),
    agentDelta: T.notification('item/agentMessage/delta', 'd6Conn1', 'item/agentMessage/delta'),
    agentItemCompleted: T.notification('item/completed agentMessage', 'd6Conn1', 'item/completed', isItem('agentMessage')),
    turnCompleted: T.notification('turn/completed', 'd6Conn1', 'turn/completed'),
    queueChanged: T.notification('thread/queue/changed', 'd6Conn1', 'thread/queue/changed'),
    queueAdd,
    loadedList,
    threadList,
    turnsList,
    steerResp,
  };
  checkShapes(t);
  return { templates: deepFreeze(t), used: Object.fromEntries(T.used), clone };
}

// Fail closed if a template lacks a member the fake writes into.
function checkShapes(t) {
  const need = (cond, what) => {
    if (!cond) throw new Error(`fixture template drift: ${what}`);
  };
  need(typeof t.initialize.userAgent === 'string' && /\([^)]*\)\s*$/.test(t.initialize.userAgent), 'initialize.userAgent ends in "(name; version)"');
  for (const [n, th] of [['thread/start', t.threadStart.thread], ['thread/resume', t.threadResume.thread]]) {
    for (const k of ['id', 'sessionId', 'preview', 'ephemeral', 'createdAt', 'updatedAt', 'recencyAt', 'status', 'path', 'cwd', 'environments', 'turns']) {
      need(k in th, `${n} result thread.${k}`);
    }
  }
  need(t.resumeFail.code === -32600 && t.resumeFail.message.includes(t.resumeFailThreadId), 'thread/resume failure names the thread id');
  for (const k of ['id', 'items', 'itemsView', 'status', 'error', 'startedAt', 'completedAt', 'durationMs']) {
    need(k in t.turnStart.turn, `turn/start result turn.${k}`);
    need(k in t.turnCompleted.params.turn, `turn/completed turn.${k}`);
  }
  need(t.queueAdd.queuedSubmission && 'clientUserMessageId' in t.queueAdd.queuedSubmission, 'thread/queue/add result queuedSubmission');
  need(Array.isArray(t.turnsList.data) && t.turnsList.data.length > 0, 'thread/turns/list result data');
  need(Array.isArray(t.loadedList.data) && 'nextCursor' in t.loadedList, 'thread/loaded/list result');
  need(Array.isArray(t.threadList.data) && 'nextCursor' in t.threadList, 'thread/list result');
}

function deepFreeze(o) {
  if (o && typeof o === 'object' && !Object.isFrozen(o)) {
    Object.freeze(o);
    for (const v of Object.values(o)) deepFreeze(v);
  }
  return o;
}
