// A file as committed at HEAD versus the working tree: shared by the driver (run.mjs reads the
// herdr pin through it, #139) and the gate scenarios. Moved out of lib/g1.mjs (#139 review) so
// the driver core does not import a scenario library; g1.mjs re-exports both names.

import { createHash } from 'node:crypto';
import { lstatSync, readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { join } from 'node:path';

export const sha256 = (buf) => createHash('sha256').update(buf).digest('hex');

// A file as COMMITTED at HEAD (git's blob, not whatever is on disk), plus whether the
// working-tree file still matches it. A locally edited, replaced or symlinked working-tree
// file therefore cannot pass as "the committed file". Throws when git cannot answer.
//
// The match is decided in git's normalized form: the working-tree file is hashed with
// `git hash-object --path`, which applies the same clean filters (core.autocrlf, eol
// attributes) `git status` does, and compared with the blob id at HEAD. A CRLF checkout of an
// LF blob (Git for Windows' default) therefore matches, as `git status` says it does (#152).
// workingTreeSha256 stays the sha256 of the raw bytes on disk, for the record.
export function committedFile(repoRoot, relPath) {
  const git = (args, encoding) => spawnSync('git', args, { cwd: repoRoot, encoding, timeout: 15000, maxBuffer: 64 * 1024 * 1024 });
  const head = git(['rev-parse', 'HEAD'], 'utf8');
  const tree = git(['ls-tree', 'HEAD', '--', relPath], 'utf8');
  const blob = git(['cat-file', 'blob', `HEAD:${relPath}`], 'buffer');
  if (head.status !== 0 || tree.status !== 0 || blob.status !== 0 || !String(tree.stdout).trim()) {
    throw new Error(`cannot read ${relPath} as committed at HEAD (git ls-tree/cat-file failed)`);
  }
  const [mode, , blobId] = String(tree.stdout).trim().split(/\s+/);
  const bytes = blob.stdout;
  let workingTreeSha256 = null;
  let workingTreeIsSymlink = null;
  let workingTreeBlobId = null;
  try {
    workingTreeIsSymlink = lstatSync(join(repoRoot, relPath)).isSymbolicLink();
    workingTreeSha256 = sha256(readFileSync(join(repoRoot, relPath)));
    if (!workingTreeIsSymlink) {
      const h = git(['hash-object', `--path=${relPath}`, '--', relPath], 'utf8');
      if (h.status === 0) workingTreeBlobId = h.stdout.trim();
    }
  } catch {
    /* missing on disk: recorded as null, and never matches */
  }
  const committedSha256 = sha256(bytes);
  return {
    path: relPath,
    headCommit: head.stdout.trim(),
    mode,
    bytes,
    committedSha256,
    workingTreeSha256,
    workingTreeIsSymlink,
    workingTreeMatchesHead: mode === '100644' || mode === '100755' ? workingTreeIsSymlink === false && workingTreeBlobId !== null && workingTreeBlobId === blobId : false,
  };
}
