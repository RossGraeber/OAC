// Canonical path containment for the driver's path guards (#353).
//
// A guard that compares paths as spelled can be walked around by a symlink: on macOS
// os.tmpdir() sits under /var, a symlink to /private/var, so a file whose realpath is
// /private/var/.../codex-home/<file> was "outside" a CODEX_HOME spelled /var/.../codex-home,
// and executableIdentity() read and hashed it (ADR-001 boundary 3). Every guard here
// compares canonical forms on both sides, and fails closed when a form cannot be computed.
//
// Canonicalizing a directory resolves its own entry (and its ancestors'); it never lists,
// opens or reads anything inside it.

import { realpathSync } from 'node:fs';
import { basename, dirname, isAbsolute, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const MISSING = new Set(['ENOENT', 'ENOTDIR']);

/**
 * Every spelling of `p` a guard must compare: as resolved, its realpath through Node's
 * resolver (which the self-test's fs trace sees) and through the OS resolver
 * (realpathSync.native: on Windows it also expands 8.3 short names). A path that does not
 * exist yet is canonicalized through its nearest existing ancestor.
 * @returns {{ spelled: string, real: string[] } | null} null when the path cannot be
 *   canonicalized (any error but "does not exist", or `mustExist` and it does not): fail closed.
 */
export function canonicalForms(p, { mustExist = false } = {}) {
  if (typeof p !== 'string' || !p) return null;
  const spelled = resolve(p);
  const real = new Set();
  let head = spelled;
  const tail = [];
  for (;;) {
    for (const fn of [realpathSync, realpathSync.native]) {
      if (typeof fn !== 'function') continue;
      try {
        real.add(resolve(fn(head), ...tail));
      } catch (err) {
        if (!MISSING.has(err?.code)) return null;
      }
    }
    if (real.size) return { spelled, real: [...real] };
    if (mustExist) return null;
    const up = dirname(head);
    if (up === head) return null;
    tail.unshift(basename(head));
    head = up;
  }
}

// `file` is `root` or below it. Both absolute. A child named `..x` is inside (a bare
// startsWith('..') test would put it outside and fail open).
export function isWithin(file, root) {
  const rel = relative(root, file);
  return rel === '' || (rel !== '..' && !rel.startsWith(`..${sep}`) && !isAbsolute(rel));
}

const all = (f) => [f.spelled, ...f.real];

/** Any form of `file` is within any form of `root` (both canonicalForms() results). */
export function formsWithin(file, root) {
  return all(file).some((a) => all(root).some((b) => isWithin(a, b)));
}

/**
 * Whether `path`, after every symlink, still names a file of the same basename. A harness
 * config file the driver hashes (config.toml, settings.json, ...) that is a symlink to a file
 * of another name -- a credential file beside it, say -- is not read (#353). A dotfiles-style
 * link to a file of the same name passes. Fails closed: false when it cannot be canonicalized.
 */
export function keepsBasename(path) {
  const f = canonicalForms(path, { mustExist: true });
  if (!f) return false;
  const want = basename(f.spelled);
  return f.real.every((r) => (process.platform === 'win32' ? basename(r).toLowerCase() === want.toLowerCase() : basename(r) === want));
}

/**
 * Whether this module is the process's entry script. Node's main module URL is its realpath,
 * so a spelled comparison with argv[1] is false when the script was started through a
 * symlinked directory (macOS /var -> /private/var: a throwaway clone under os.tmpdir()), and
 * the CLI silently did nothing and exited 0 (#353).
 */
export function isMainModule(metaUrl, argv1 = process.argv[1]) {
  if (!argv1) return false;
  try {
    return realpathSync(resolve(argv1)) === realpathSync(fileURLToPath(metaUrl));
  } catch {
    return false;
  }
}

/**
 * Whether `path` is within `root`, canonicalized on both sides. Fails closed: when either
 * side cannot be canonicalized the answer is true (treated as inside).
 */
export function canonicallyWithin(path, root) {
  const f = canonicalForms(path);
  const r = canonicalForms(root);
  return !f || !r || formsWithin(f, r);
}
