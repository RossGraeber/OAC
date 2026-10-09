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
//
// Two directions (#357): a guard that refuses a read (canonicallyWithin: "is this inside a
// harness home?") answers "inside" on any error; a guard that allows one (canonicallyInside:
// "is this inside the one directory we may read?") answers "not inside" on any error. Both
// fail closed.
//
// Residuals (documented in tools/herdr/README.md): a hard link to a file inside a home has no
// path relation to it (the L3 session-file read skips any file with more than one link, #357);
// an overlayfs upperdir/workdir spelling of a merged home (see the identity section); a
// same-filesystem bind mount of a single file into the L3 slug directory (a bind from another
// filesystem is skipped by its st_dev, #357); and the check and the later open are separate calls, so a directory on the path swapped for
// a link in between (TOCTOU) would redirect the open.

import { realpathSync, statSync } from 'node:fs';
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

// --- identity (PR #356 review B1) ------------------------------------------------------
//
// Spellings are not enough: neither realpath maps a Windows UNC admin-share spelling
// (\\localhost\C$\...) back to its drive letter, and neither resolves a Linux bind mount, so
// either spelling of a harness home walked around the forms check. The guards therefore also
// compare file identity: the (dev, ino) of a root's own entry, stat'ed once with bigint, against
// the (dev, ino) of the target and each of its ancestors. A stat error other than "does not
// exist" fails closed. Statting the root reads its entry's metadata only; nothing inside it is
// listed or read, and the ancestor walk goes top down and stops at the first match, so it never
// stats an entry below a matching root.
//
// A filesystem that reports ino 0 (FAT/exFAT, some network shares) has no usable identity.
// Before #357 such a root fell back to the spelling checks alone, which reopens #353 there; now
// a root without identity cannot be guarded (rootGuard() is null: the caller fails closed), and
// an ino-0 entry on a target's chain is an error (fail closed).
//
// overlayfs (#357, PR #358 review B1): a directory that exists in lowerdir reports, in the
// merged view, the lowerdir directory's st_ino on the overlay's own st_dev, so a (dev, ino)
// match misses merged vs lowerdir. A harness-home guard (harnessHomeGuard()) therefore also
// refuses an ino-only match on another device. That covers merged vs lowerdir only: the
// upperdir copy of a directory has an inode of its own, which no merged entry reports, so
// merged vs upperdir (and workdir) is a residual, like the hard link; making that view needs
// mount privilege. A false positive of the ino-only rule (a home that is a filesystem or
// subvolume root: ext4 ino 2, btrfs ino 256, matching another such root on an executable's
// path) only leaves a file unhashed, and guardMatch() names it 'inode-other-device' so the
// caller records a finding. The repository guard keeps the (dev, ino) match, where a false
// positive would refuse a run.
//
// `stat` is injectable for the self-test only (an ino-0 filesystem cannot be made in a temp dir).

/**
 * The identity of `p`'s own entry (following symlinks): { dev, ino } as bigints, or null when
 * it does not exist. Throws on any other stat error, and when the filesystem reports no inode
 * (ino 0, code ENOINO: identity is unavailable there, #357).
 */
export function entryIdentity(p, { stat = statSync } = {}) {
  let s;
  try {
    s = stat(p, { bigint: true });
  } catch (err) {
    if (MISSING.has(err?.code)) return null;
    throw err;
  }
  if (s.ino === 0n) throw Object.assign(new Error('the filesystem reports no inode (ino 0): file identity is unavailable (#357)'), { code: 'ENOINO' });
  return { dev: s.dev, ino: s.ino };
}

/**
 * How any form of `file` (a canonicalForms() result), or any ancestor of one, matches one of
 * the identities `ids`: 'identity' (same dev and ino), 'inode-other-device' (`anyDevice` only:
 * the same ino on another device, and no exact match), 'error' (a stat error other than "does
 * not exist", or an entry reporting ino 0), or null (no match). Each chain is walked from the
 * filesystem root down; an entry that does not exist (yet) is skipped.
 */
export function identityMatch(file, ids, { anyDevice = false, stat = statSync } = {}) {
  if (!ids.length) return null;
  let loose = null;
  for (const start of all(file)) {
    const chain = [];
    for (let p = start; ; ) {
      chain.unshift(p);
      const up = dirname(p);
      if (up === p) break;
      p = up;
    }
    for (const p of chain) {
      let s;
      try {
        s = stat(p, { bigint: true });
      } catch (err) {
        if (MISSING.has(err?.code)) continue;
        return 'error';
      }
      if (s.ino === 0n) return 'error';
      if (ids.some((i) => i.ino === s.ino && i.dev === s.dev)) return 'identity';
      if (anyDevice && ids.some((i) => i.ino === s.ino)) loose = 'inode-other-device';
    }
  }
  return loose;
}

/**
 * Whether `file` matches `ids` by identityMatch(); an error answers `onError` (true for a
 * refusing guard, false for an allowing one: both fail closed).
 */
export function identityWithin(file, ids, { onError = true, anyDevice = false, stat = statSync } = {}) {
  const m = identityMatch(file, ids, { anyDevice, stat });
  return m === 'error' ? onError : m !== null;
}

/**
 * The spellings and identity of a root (a harness home, the repository): { forms, ids,
 * anyDevice }, or null when it cannot be canonicalized or stat'ed, or reports no identity
 * (ino 0, #357): the caller fails closed.
 */
export function rootGuard(root, { anyDevice = false, stat = statSync } = {}) {
  const forms = canonicalForms(root);
  if (!forms) return null;
  try {
    const id = entryIdentity(forms.real[0] ?? forms.spelled, { stat });
    return { forms, ids: id ? [id] : [], anyDevice, stat };
  } catch {
    return null;
  }
}

/**
 * Why a root cannot be guarded, or null when rootGuard() would succeed: for an error message
 * that names the cause (#357: a repository on a filesystem without file identity is refused
 * with that reason, not as a path "inside the repository").
 */
export function rootGuardProblem(root, { stat = statSync } = {}) {
  const forms = canonicalForms(root);
  if (!forms) return 'cannot be canonicalized (realpath failed)';
  try {
    entryIdentity(forms.real[0] ?? forms.spelled, { stat });
    return null;
  } catch (err) {
    if (err?.code === 'ENOINO') return 'is on a filesystem that reports no file identity (ino 0, e.g. FAT/exFAT or some network shares), so the containment check cannot compare identities (#357)';
    return `cannot be stat'ed (${err?.code ?? 'error'})`;
  }
}

/** A harness home's guard: rootGuard() that also refuses an ino-only match on another device (overlayfs merged vs lowerdir, #357). */
export const harnessHomeGuard = (home, opts = {}) => rootGuard(home, { ...opts, anyDevice: true });

/**
 * How `file` (a canonicalForms() result) is inside `guard` (a rootGuard() result): 'spelling',
 * 'identity', 'inode-other-device' (a harness-home guard's ino-only match), 'error' (counts as
 * inside: a refusing guard), or null (outside).
 */
export function guardMatch(file, guard) {
  if (formsWithin(file, guard.forms)) return 'spelling';
  return identityMatch(file, guard.ids, { anyDevice: guard.anyDevice, stat: guard.stat });
}

/** `file` (a canonicalForms() result) is inside `guard` (a rootGuard() result), by spelling or identity; errors answer inside. */
export function guardHolds(file, guard) {
  return guardMatch(file, guard) !== null;
}

/**
 * Whether `path` is within `root`, by spelling (canonicalized on both sides) or by file
 * identity. For a guard that refuses: when either side cannot be canonicalized or stat'ed, or
 * reports no identity, the answer is true (treated as inside).
 */
export function canonicallyWithin(path, root, { stat = statSync } = {}) {
  const f = canonicalForms(path);
  const r = rootGuard(root, { stat });
  return !f || !r || guardHolds(f, r);
}

/**
 * Whether `path` (which must exist) is verifiably within `root`, by spelling or by file
 * identity. For a guard that allows a read (#357): any error -- either side not
 * canonicalized, a stat error, no identity (ino 0) -- answers false (not inside, not read).
 */
export function canonicallyInside(path, root, { stat = statSync } = {}) {
  const f = canonicalForms(path, { mustExist: true });
  const r = rootGuard(root, { stat });
  if (!f || !r) return false;
  return formsWithin(f, r.forms) || identityWithin(f, r.ids, { onError: false, stat });
}
