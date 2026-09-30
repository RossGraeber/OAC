// Test double for #202: preloaded into the driver process (`node --import <this> run.mjs`)
// by the self-test, it makes removal of the driver's own scratch directory throw EPERM, the
// way a transient Windows file lock (Defender, the indexer) does during a recursive delete.
//
//   FAKE_RM_EPERM=once    the first removal attempt throws; later attempts run for real
//   FAKE_RM_EPERM=always  every attempt throws; the directory is left for the test to delete
//
// Only paths whose last segment starts with `oac-herdr-scratch-` are affected; every other
// rmSync call runs unchanged. It patches node:fs in this process only (not its children).

import fs from 'node:fs';
import { syncBuiltinESMExports } from 'node:module';
import { basename } from 'node:path';

const mode = process.env.FAKE_RM_EPERM;
const realRmSync = fs.rmSync;
let thrown = 0;

fs.rmSync = function rmSync(path, options) {
  const p = String(path);
  if (basename(p).startsWith('oac-herdr-scratch-') && (mode === 'always' || (mode === 'once' && thrown === 0))) {
    thrown += 1;
    const err = new Error(`EPERM, Permission denied: \\\\?\\${p} '\\\\?\\${p}'`);
    err.code = 'EPERM';
    err.errno = -4048;
    err.syscall = 'rmdir';
    err.path = p;
    throw err;
  }
  return realRmSync.call(this, path, options);
};
syncBuiltinESMExports();
