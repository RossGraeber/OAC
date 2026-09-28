// TEST-ONLY preload for the K7 self-test (tools/herdr/test/g2-tests.mjs), loaded with
// NODE_OPTIONS=--import=<this file's URL> into the driver and every Node process it starts.
// It records, to $OAC_FS_TRACE_FILE (JSONL), every path the process reads, opens, stats or
// lists through node:fs, and every child process it starts through node:child_process,
// tagged with the process's pid and script. The self-test then checks that neither the
// driver (run.mjs) nor the staged G2 client opened anything under the Codex home beyond the
// two harness-config files run.mjs hashes, and that neither started anything unexpected. K8
// (#131) adds write/move/delete operations (kind 'write') and reuses it for G4 and G5.
//
// It only observes: every wrapped function calls the original with the same arguments.
// ESM named imports of node:fs / node:child_process see the wrappers because
// syncBuiltinESMExports() updates the builtin modules' live bindings.

import { createRequire, syncBuiltinESMExports } from 'node:module';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const fs = require('node:fs');
const cp = require('node:child_process');
const OUT = process.env.OAC_FS_TRACE_FILE;

if (OUT) {
  const append = fs.appendFileSync;
  const script = process.argv[1] ?? null;
  let busy = false; // appendFileSync itself goes through fs.openSync: no recursion
  const rec = (o) => {
    if (busy) return;
    busy = true;
    try {
      append(OUT, `${JSON.stringify({ pid: process.pid, script, ...o })}\n`);
    } catch {
      /* tracing must never change behavior */
    } finally {
      busy = false;
    }
  };
  const pathOf = (p) => {
    try {
      if (typeof p === 'string') return resolve(p);
      if (p instanceof URL) return fileURLToPath(p);
      if (Buffer.isBuffer(p)) return resolve(p.toString());
    } catch {
      /* not a path */
    }
    return null;
  };
  const wrap = (obj, name, kind, describe) => {
    const orig = obj[name];
    if (typeof orig !== 'function') return;
    const wrapped = function (...a) {
      const d = describe(a);
      if (d) rec({ kind, op: name, ...d });
      return orig.apply(this, a);
    };
    for (const k of Object.getOwnPropertyNames(orig)) {
      // e.g. realpathSync.native, promisify.custom-style extras
      if (!(k in wrapped)) {
        try {
          wrapped[k] = orig[k];
        } catch {
          /* read-only */
        }
      }
    }
    obj[name] = wrapped;
  };
  const byPath = (a) => {
    const p = pathOf(a[0]);
    return p ? { path: p } : null;
  };
  const FS = ['readFileSync', 'readFile', 'openSync', 'open', 'createReadStream', 'existsSync', 'statSync', 'stat', 'lstatSync', 'lstat', 'readdirSync', 'readdir', 'accessSync', 'access', 'realpathSync', 'realpath', 'opendirSync', 'opendir', 'readlinkSync', 'readlink', 'copyFileSync', 'copyFile', 'cpSync', 'cp'];
  for (const n of FS) wrap(fs, n, 'fs', byPath);
  for (const n of ['readFile', 'open', 'stat', 'lstat', 'readdir', 'access', 'realpath', 'opendir', 'readlink', 'copyFile', 'cp']) wrap(fs.promises, n, 'fs', byPath);
  // K8 (#131): writes, moves and deletes too (kind 'write'), so a test can show no process
  // wrote under a harness home or created a harness config file anywhere.
  const WRITE = ['writeFileSync', 'writeFile', 'appendFileSync', 'appendFile', 'renameSync', 'rename', 'rmSync', 'rm', 'unlinkSync', 'unlink', 'mkdirSync', 'mkdir', 'symlinkSync', 'symlink', 'linkSync', 'link', 'truncateSync', 'truncate', 'createWriteStream'];
  for (const n of WRITE) wrap(fs, n, 'write', byPath);
  for (const n of ['writeFile', 'appendFile', 'rename', 'rm', 'unlink', 'mkdir', 'symlink', 'link', 'truncate']) wrap(fs.promises, n, 'write', byPath);
  const bySpawn = (a) => ({ file: String(a[0]), args: Array.isArray(a[1]) ? a[1].slice(0, 8).map(String) : null });
  for (const n of ['spawn', 'spawnSync', 'execFile', 'execFileSync', 'exec', 'execSync', 'fork']) wrap(cp, n, 'spawn', bySpawn);
  syncBuiltinESMExports();
}
