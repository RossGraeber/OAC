// Scratch-directory removal for the herdr driver (#202). Never throws: a run's manifest must
// survive a cleanup failure. On Windows a recursive delete can fail with EPERM/EBUSY while
// another process holds a handle under scratch, so removal is retried with a bounded
// backoff; a failure that outlasts it is returned for the caller to record as a leftover path
// and a finding, not raised. It only ever removes the driver's own scratch directory: a path
// that is not directly under os.tmpdir() with an mkdtemp `oac-herdr-scratch-XXXXXX` name is
// refused (code EREFUSED) without calling the remover.

import { rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, resolve } from 'node:path';

// Attempts and delays (ms) between them: at most 5 attempts, about 3 s of waiting in all,
// on top of Node's own per-call retries (maxRetries/retryDelay, which cover EBUSY, EMFILE,
// ENFILE, ENOTEMPTY and EPERM).
export const SCRATCH_RETRY_DELAYS_MS = Object.freeze([200, 400, 800, 1600]);

export const SCRATCH_NAME = /^oac-herdr-scratch-[A-Za-z0-9]{6}$/;

// Declared (ctx.noteScratchHolder) by scenarios that start or use the shared Codex
// app-server daemon with a thread whose cwd is under scratch. Observed on Windows (#202,
// L3 probe runs 2026-09-30 19:04Z and 19:09Z): while the daemon runs, removing scratch
// fails EPERM; after `codex app-server daemon stop` the same delete succeeds. The driver
// never stops or restarts the operator's daemon, and it calls no app-server method to
// release the thread:
// - `thread/unsubscribe` (stable in the rust-v0.159.2 schema, ClientRequest.json) keeps the
//   thread loaded "until it has no subscribers and no thread activity for 30 minutes"
//   (https://learn.chatgpt.com/docs/app-server, retrieved 2026-09-30); the driver's client
//   connections have already closed by teardown (a disconnect unsubscribes).
// - `thread/archive` and `thread/delete` do unload a loaded thread promptly: at rust-v0.159.2
//   both go through `prepare_thread_for_removal`
//   (codex-rs/app-server/src/request_processors/thread_processor.rs), which removes the
//   loaded thread and waits for its shutdown. They are not called because both write to the
//   operator's persistent Codex thread store, `thread/delete` destroys the run's rollout, and
//   whether that shutdown releases the Windows directory handle is UNVERIFIED.
export const CODEX_DAEMON_SCRATCH_HOLDER =
  'the shared Codex app-server daemon (left running; the driver never stops it) may hold the directory of a thread it loaded under scratch; ' +
  'it is released on `codex app-server daemon stop`, or when the daemon unloads the thread (documented: no subscribers and no thread activity for 30 minutes). ' +
  'The driver does not call thread/archive or thread/delete, which would unload it sooner, because both write to the operator\'s Codex thread store ' +
  '(thread/delete destroys the run\'s rollout) and whether their shutdown releases the directory handle is unverified';

const sleep = (ms) => new Promise((res) => setTimeout(res, ms));

export function defaultRemove(path) {
  rmSync(path, { recursive: true, force: true, maxRetries: 2, retryDelay: 100 });
}

const norm = (p) => (process.platform === 'win32' ? p.toLowerCase() : p);

// Why `path` is not the driver's own scratch directory, or null if it is.
export function scratchRefusal(path, { tmp = tmpdir() } = {}) {
  if (typeof path !== 'string' || !path) return 'not a path';
  if (path.split(/[\\/]+/).includes('..')) return 'path has a ".." segment';
  const abs = resolve(path);
  if (norm(dirname(abs)) !== norm(resolve(tmp))) return 'not directly under os.tmpdir()';
  if (!SCRATCH_NAME.test(basename(abs))) return 'name is not oac-herdr-scratch-XXXXXX';
  return null;
}

// Returns { removed, attempts, errors: [{ attempt, code, message }] }. `remove`, `wait` and
// `tmp` are injectable for tests; the defaults are rmSync with retries, a real timer and
// os.tmpdir(). A refused path returns { removed: false, attempts: 0, errors: [EREFUSED] }.
export async function removeScratch(path, { remove = defaultRemove, delaysMs = SCRATCH_RETRY_DELAYS_MS, wait = sleep, tmp } = {}) {
  const refusal = scratchRefusal(path, tmp === undefined ? {} : { tmp });
  if (refusal) return { removed: false, attempts: 0, errors: [{ attempt: 0, code: 'EREFUSED', message: `refused to remove ${path}: ${refusal}` }] };
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
