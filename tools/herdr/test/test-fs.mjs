// Windows-safe filesystem helpers for the herdr self-test's scratch directories (#400).
//
// Two Windows facts, both measured (#400), shape these:
//   - renameSync(tmp, existing) fails with EPERM while another process has `existing` open,
//     even one that only reads it (fake-herdr polls the harness doubles' `status` file every
//     50 ms): 745 of 3000 replacements failed with one polling reader, none with no reader.
//     renameReplace retries that for up to 2 s; a failure that outlasts it is thrown.
//   - a delete fails with EPERM while a process has the directory as its working directory.
//     That is the test's own doing when a child is still running, so the tests stop and await
//     their own children first (wait-tests fakeSession); what is left is a holder the test does
//     not control (antivirus, the indexer). removeTestDir retries, and if the directory still
//     cannot be removed it records a failed check naming the path and the error, never a throw
//     (which, from a `finally`, would also hide the error that sent control there).

import { renameSync, rmSync } from 'node:fs';

const sleepSync = (ms) => Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
const TRANSIENT = new Set(['EPERM', 'EBUSY', 'EACCES']);

export function renameReplace(from, to, { rename = renameSync, platform = process.platform, tries = 100, delayMs = 20, sleep = sleepSync } = {}) {
  for (let i = 1; ; i++) {
    try {
      rename(from, to);
      return i;
    } catch (err) {
      if (platform !== 'win32' || !TRANSIENT.has(err?.code) || i >= tries) throw err;
      sleep(delayMs);
    }
  }
}

// Node's own retries (maxRetries 10, retryDelay 100: linear backoff, about 5.5 s) cover EBUSY,
// EMFILE, ENFILE, ENOTEMPTY and EPERM. Returns true when the directory is gone; otherwise it
// records one failed check (`label`) with the error code and message, and returns false.
export function removeTestDir(path, check, label, { remove = rmSync } = {}) {
  try {
    remove(path, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
    return true;
  } catch (err) {
    check(`${label}: scratch directory removed (left behind if not: antivirus or another process outside the test holds it)`, false, `${err?.code ?? 'error'}: ${String(err?.message ?? err)}`);
    return false;
  }
}
