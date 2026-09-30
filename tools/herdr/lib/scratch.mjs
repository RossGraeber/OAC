// Scratch-directory removal for the herdr driver (#202). Never throws: a run's manifest must
// survive a cleanup failure. On Windows a recursive delete can fail transiently with
// EPERM/EBUSY while Defender or the indexer holds a file open, so removal is retried with a
// bounded backoff; a failure that outlasts it is returned for the caller to record as a
// leftover path and a finding, not raised.

import { rmSync } from 'node:fs';

// Attempts and delays (ms) between them: at most 5 attempts, about 3 s of waiting in all,
// on top of Node's own per-call retries (maxRetries/retryDelay, which cover EBUSY, EMFILE,
// ENFILE, ENOTEMPTY and EPERM).
export const SCRATCH_RETRY_DELAYS_MS = Object.freeze([200, 400, 800, 1600]);

// Declared (ctx.noteScratchHolder) by scenarios that start or use the shared Codex
// app-server daemon with a thread whose cwd is under scratch. Observed on Windows (#202,
// L3 probe runs 2026-09-30 19:04Z and 19:09Z): while the daemon runs, removing scratch
// fails EPERM; after `codex app-server daemon stop` the same delete succeeds. The driver
// never stops or restarts the operator's daemon. The only documented release call,
// `thread/unsubscribe` (stable in the 0.159.2 schema, ClientRequest.json), keeps the thread
// loaded "until it has no subscribers and no thread activity for 30 minutes"
// (https://learn.chatgpt.com/docs/app-server, retrieved 2026-09-30), and the driver's own
// client connections have already closed (a disconnect unsubscribes), so calling it would
// not release the directory within the run; the driver does not call it, and no other
// documented method unloads a thread promptly.
export const CODEX_DAEMON_SCRATCH_HOLDER =
  'the shared Codex app-server daemon (left running; the driver never stops it) may hold the directory of a thread it loaded under scratch; ' +
  'it is released on `codex app-server daemon stop`, or when the daemon unloads the thread (documented: no subscribers and no thread activity for 30 minutes)';

const sleep =(ms) => new Promise((res) => setTimeout(res, ms));

export function defaultRemove(path) {
  rmSync(path, { recursive: true, force: true, maxRetries: 2, retryDelay: 100 });
}

// Returns { removed, attempts, errors: [{ attempt, code, message }] }. `remove` and `wait`
// are injectable for tests; the defaults are rmSync with retries and a real timer.
export async function removeScratch(path, { remove = defaultRemove, delaysMs = SCRATCH_RETRY_DELAYS_MS, wait = sleep } = {}) {
  const errors = [];
  for (let attempt = 1; attempt <= delaysMs.length + 1; attempt++) {
    try {
      remove(path);
      return { removed: true, attempts: attempt, errors };
    } catch (err) {
      errors.push({ attempt, code: err?.code ?? null, message: String(err?.message ?? err) });
      if (attempt <= delaysMs.length) await wait(delaysMs[attempt - 1]);
    }
  }
  return { removed: false, attempts: errors.length, errors };
}
