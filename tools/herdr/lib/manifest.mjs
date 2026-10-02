// Run-manifest helpers: driver commit, OS, harness versions, harness config hashes, and the
// launch-environment delta. Nothing here writes outside the run's own directories.
//
// Harness config is READ AND HASHED ONLY (sha256), never copied, printed, or written:
// ADR-001 boundaries 3/13 and oac-boundaries check 10. The config paths are spelled out
// once, in harnessConfigFiles(); in the driver they are handed only to the hash reader in
// hashHarnessConfig(). (The self-test also calls harnessConfigFiles() to plant synthetic
// files in its own temp dirs, behind a path guard -- see test/selftest.mjs makeBase().)
// harnessConfigDirs() names their directories only so executableIdentity() can refuse to
// read anything there but a command's own managed binary (#140).
//
// Environment VALUES are never read here except for the allowlist in env-probe.mjs
// (HERDR_*, TERM, COLORTERM). Everything else is compared by variable NAME only, so a
// provider credential that happens to sit in the operator's environment is never read.

import { createHash } from 'node:crypto';
import { readFileSync, existsSync, statSync, accessSync, realpathSync, createReadStream, constants as fsConstants } from 'node:fs';
import { homedir, platform, release, arch, type } from 'node:os';
import { basename, isAbsolute, join, posix, relative, resolve, win32 } from 'node:path';
import { spawnSync } from 'node:child_process';
import { runBounded } from './proc.mjs';

// 2 (#140): herdr.executable, harnessExecutables, captures[].sha256.
export const MANIFEST_SCHEMA_VERSION = 2;

export function driverInfo(repoRoot) {
  const git = (args) => spawnSync('git', args, { cwd: repoRoot, encoding: 'utf8', timeout: 10000 });
  const head = git(['rev-parse', 'HEAD']);
  const dirty = git(['status', '--porcelain', '--', 'tools/herdr']);
  return {
    entry: 'tools/herdr/run.mjs',
    commit: head.status === 0 ? head.stdout.trim() : null,
    toolsHerdrDirty: dirty.status === 0 ? dirty.stdout.trim().length > 0 : null,
    node: process.version,
  };
}

// No hostname and no username: both are redaction targets, not manifest fields.
export function osInfo() {
  return { platform: platform(), type: type(), release: release(), arch: arch() };
}

function sha256File(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex');
}

// The harness config files whose before/after hashes every run records.
export function harnessConfigFiles(env = process.env) {
  const claudeDir = env.CLAUDE_CONFIG_DIR || join(homedir(), '.claude');
  const codexDir = env.CODEX_HOME || join(homedir(), '.codex');
  const claudeLabel = env.CLAUDE_CONFIG_DIR ? '$CLAUDE_CONFIG_DIR' : '~/.claude';
  const codexLabel = env.CODEX_HOME ? '$CODEX_HOME' : '~/.codex';
  return [
    { label: `${claudeLabel}/settings.json`, path: join(claudeDir, 'settings.json') },
    { label: `${codexLabel}/config.toml`, path: join(codexDir, 'config.toml') },
    { label: `${codexLabel}/hooks.json`, path: join(codexDir, 'hooks.json') },
  ];
}

export function hashHarnessConfig(env = process.env) {
  return harnessConfigFiles(env).map(({ label, path }) => {
    if (!existsSync(path)) return { file: label, present: false, sha256: null };
    try {
      return { file: label, present: true, sha256: sha256File(path) };
    } catch (err) {
      return { file: label, present: true, sha256: null, error: err.code || 'read failed' };
    }
  });
}

export function compareHashes(before, after) {
  const changed = [];
  for (const b of before) {
    const a = after.find((x) => x.file === b.file);
    if (!a || a.present !== b.present || a.sha256 !== b.sha256) changed.push(b.file);
  }
  return { unchanged: changed.length === 0, changed };
}

// --- executable identity (#140) ---------------------------------------------------------
//
// The run manifest records which herdr and which harness executables ran: the file's
// basename (never its directory -- a home path or user name is not a manifest field), the
// basename after symlinks, its sha256 and size, and its format by magic bytes. Nothing is
// read from a harness config directory: an executable that resolves inside one is recorded
// unhashed (ADR-001 boundary 3; the config files there are hashed only by
// hashHarnessConfig()).

const WIN_DEFAULT_PATHEXT = '.COM;.EXE;.BAT;.CMD';
const SCRIPT_EXT = /\.(?:cmd|bat|ps1|js|mjs|cjs|sh)$/i;

// Resolve a command name the way the platform's PATH search would, against `env.PATH`.
// win32: each PATH entry, each PATHEXT extension in order (or `exts`, e.g. the .com/.exe
// that a shell-less spawn can run); a name that already carries one of those extensions is
// tried as is first. POSIX: the first PATH entry holding an executable regular file. A name
// with a path separator is resolved against `cwd` instead. Returns the absolute path or null.
export function resolveExecutable(name, { env = process.env, platform = process.platform, exts, cwd = process.cwd() } = {}) {
  const isWin = platform === 'win32';
  const p = isWin ? win32 : posix;
  const pathVar = isWin ? Object.entries(env).find(([k]) => k.toUpperCase() === 'PATH')?.[1] : env.PATH;
  const extList = isWin ? (exts ?? (env.PATHEXT || env.Pathext || WIN_DEFAULT_PATHEXT).split(';').filter(Boolean)) : [''];
  const candidates = (base) => {
    if (!isWin) return [base];
    const hasExt = extList.some((e) => base.toLowerCase().endsWith(e.toLowerCase()));
    return [...(hasExt ? [base] : []), ...extList.map((e) => base + e.toLowerCase())];
  };
  const usable = (f) => {
    try {
      if (!statSync(f).isFile()) return false;
      if (!isWin) accessSync(f, fsConstants.X_OK);
      return true;
    } catch {
      return false;
    }
  };
  const hasSep = isWin ? /[\\/]/.test(name) : name.includes('/');
  if (hasSep) return candidates(p.resolve(cwd, name)).find(usable) ?? null;
  for (const dir of (pathVar ?? '').split(isWin ? ';' : ':')) {
    const d = dir.replace(/^"(.*)"$/, '$1');
    if (!d || !p.isAbsolute(d)) continue; // a relative PATH entry depends on the cwd: skipped
    const hit = candidates(p.join(d, name)).find(usable);
    if (hit) return hit;
  }
  return null;
}

// The harness config directories (the parents of harnessConfigFiles()).
function harnessConfigDirs(env = process.env) {
  return [env.CLAUDE_CONFIG_DIR || join(homedir(), '.claude'), env.CODEX_HOME || join(homedir(), '.codex')];
}

// Compared as spelled: the config directory itself is never touched, not even by realpath
// (the self-test's fs trace holds the driver to that).
function isUnder(file, dir) {
  const rel = relative(resolve(dir), file);
  return rel === '' || (!rel.startsWith('..') && !isAbsolute(rel));
}

// Format by magic bytes: a native executable ('elf', 'pe', 'mach-o') or a 'script' (a #!
// line, or a .cmd/.bat/.ps1/.js/.mjs/.cjs/.sh file); otherwise 'unknown'.
export function executableFormat(head, file = '') {
  if (head.length >= 4 && head[0] === 0x7f && head[1] === 0x45 && head[2] === 0x4c && head[3] === 0x46) return 'elf';
  if (head.length >= 2 && head[0] === 0x4d && head[1] === 0x5a) return 'pe';
  if (head.length >= 4 && [0xfeedface, 0xfeedfacf, 0xcefaedfe, 0xcffaedfe, 0xcafebabe, 0xbebafeca].includes(head.readUInt32BE(0))) return 'mach-o';
  if (head.length >= 2 && head[0] === 0x23 && head[1] === 0x21) return 'script';
  return SCRIPT_EXT.test(file) ? 'script' : 'unknown';
}
export const NATIVE_FORMATS = Object.freeze(['elf', 'pe', 'mach-o']);

async function sha256Stream(path) {
  const hash = createHash('sha256');
  let bytes = 0;
  let head = Buffer.alloc(0);
  await new Promise((res, rej) => {
    const s = createReadStream(path);
    s.on('data', (chunk) => {
      if (head.length < 4) head = Buffer.concat([head, chunk.subarray(0, 4 - head.length)]);
      bytes += chunk.length;
      hash.update(chunk);
    });
    s.on('error', rej);
    s.on('end', res);
  });
  return { sha256: hash.digest('hex'), bytes, head };
}

// Identity of one resolved executable. `path` is absolute (or null when it did not resolve).
export async function executableIdentity(path, { requested, env = process.env } = {}) {
  if (!path) return { requested, resolved: false, basename: null, realBasename: null, sha256: null, bytes: null, format: null, error: `${requested} not found` };
  let real = path;
  try {
    real = realpathSync(path);
  } catch {
    /* recorded as spelled */
  }
  const id = { requested, resolved: true, basename: basename(path), realBasename: basename(real), sha256: null, bytes: null, format: null };
  // A harness may keep its managed binary under its config directory (Codex's standalone
  // install: a launcher directory linked to $CODEX_HOME/packages/standalone/releases/<v>/bin/).
  // That one file -- named for the requested command, never a config or credential file -- is
  // hashed; anything else there is not read.
  const ownBinary = new RegExp(`^${requested.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}(?:\\.exe|\\.com)?$`, 'i');
  if (harnessConfigDirs(env).some((d) => isUnder(real, d)) && !ownBinary.test(basename(real))) {
    return { ...id, notRead: 'resolves inside a harness config directory to a file not named for the command; never read (ADR-001 boundary 3)' };
  }
  try {
    const { sha256, bytes, head } = await sha256Stream(real);
    return { ...id, sha256, bytes, format: executableFormat(head, real) };
  } catch (err) {
    return { ...id, error: `not hashed: ${err.code || 'read failed'}` };
  }
}

// The herdr executable the driver spawns: herdrCommand()'s [file, ...prefix]. A .mjs/.js
// --herdr-bin runs under node: that is the self-test's fake herdr, so `testDouble` is true.
// `spawnPath` is what the driver should spawn (the resolved absolute path), so the hash is of
// the file that ran, not of a later PATH lookup.
export function resolveHerdr(herdrCmd, { env = process.env } = {}) {
  const runUnderNode = herdrCmd.length > 1;
  const target = runUnderNode ? herdrCmd[1] : herdrCmd[0];
  const path = runUnderNode ? target : resolveExecutable(target, { env, exts: ['.com', '.exe'] });
  return { path, runUnderNode, requested: runUnderNode ? basename(target) : /[\\/]/.test(target) ? basename(target) : target };
}

export async function herdrIdentity(resolved, { env = process.env } = {}) {
  const id = await executableIdentity(resolved.path, { requested: resolved.requested, env });
  const testDouble = resolved.runUnderNode;
  return { ...id, runUnderNode: resolved.runUnderNode, testDouble };
}

// %SystemRoot%\System32\cmd.exe from the env (case-insensitive key), or null.
export function windowsCmd(env = process.env) {
  const root = Object.entries(env).find(([k]) => k.toUpperCase() === 'SYSTEMROOT')?.[1];
  return root && win32.isAbsolute(root) ? win32.join(root, 'System32', 'cmd.exe') : null;
}

// One harness `--version` probe: resolve the name on PATH, run `--version` on the resolved
// file (win32: a .exe/.com directly, anything else through cmd.exe with a quoted path), and
// hash that same file. Bounded; a missing binary is recorded as N/A, never guessed.
async function probeHarness(h, { env, deadlineMs }) {
  const path = resolveExecutable(h, { env });
  if (!path) return { version: `N/A (not runnable: not found on PATH)`, path: null };
  let file;
  let args;
  let verbatim = false;
  if (process.platform !== 'win32' || /\.(?:exe|com)$/i.test(path)) [file, args] = [path, ['--version']];
  else if (/["%]/.test(path)) return { version: 'N/A (not runnable: path not safe to pass to cmd.exe)', path };
  else {
    // cmd.exe by full path (a bare name would be searched in the cwd first); /d no AutoRun,
    // /v:off no delayed expansion (a `!` in the path stays literal).
    const comspec = windowsCmd(env);
    if (!comspec) return { version: 'N/A (not runnable: SystemRoot is not set, so cmd.exe cannot be located)', path };
    [file, args, verbatim] = [comspec, ['/d', '/v:off', '/s', '/c', `""${path}" --version"`], true];
  }
  const r = await runBounded(file, args, { deadlineMs, env, windowsVerbatimArguments: verbatim });
  const version = r.spawnError || r.exitCode !== 0 ? `N/A (${r.spawnError ? `not runnable: ${r.spawnError}` : r.timedOut ? 'timed out' : `exit ${r.exitCode}`})` : r.stdout.trim().split('\n')[0];
  return { version, path };
}

// `<harness> --version` and the identity of the executable that answered it, for each
// harness the scenario launches (#140).
export async function probeHarnesses(harnesses, { env = process.env, deadlineMs = 15000 } = {}) {
  const versions = {};
  const executables = {};
  for (const h of harnesses) {
    const { version, path } = await probeHarness(h, { env, deadlineMs });
    versions[h] = version;
    executables[h] = await executableIdentity(path, { requested: h, env });
  }
  return { versions, executables };
}

// `<harness> --version` only (scenarios re-check versions after a run).
export async function harnessVersions(harnesses, opts = {}) {
  return (await probeHarnesses(harnesses, opts)).versions;
}

// sha256 of a capture's bytes as written (UTF-8), for manifest.captures[] (#140).
export function sha256Text(text) {
  return createHash('sha256').update(Buffer.from(text, 'utf8')).digest('hex');
}

// An enclosing Claude Code session's own variables (#163). When the driver is started from a
// shell inside a Claude Code session, these would reach the harness under test through the
// herdr server and pane: it would run as that session's child (no transcript, its permission
// mode) and could reach models through the enclosing app's relay (ANTHROPIC_BASE_URL) instead
// of its own sign-in (README "Reuse contract" point 6). No other ANTHROPIC_* variable is
// matched: those belong to the operator and pass through untouched (operator decision on
// #163).
export const HOST_HARNESS_ENV = /^(?:CLAUDECODE|CLAUDE_CODE_.*|CLAUDE_PID|CLAUDE_AGENT_SDK_.*|CLAUDE_PREVIEW_.*|ANTHROPIC_BASE_URL)$/i;

// The env the driver hands to every herdr process: the operator's environment unchanged,
// except that inherited HERDR_* variables are removed (so the driver can never talk to an
// enclosing herdr session through HERDR_SOCKET_PATH), an enclosing Claude Code session's
// variables are removed (HOST_HARNESS_ENV), and HERDR_CONFIG_PATH points at the run's own
// herdr config. Returns the env and its delta (names only; the added value is a path).
export function herdrLaunchEnv(baseEnv, configPath) {
  const env = {};
  const removed = [];
  for (const [k, v] of Object.entries(baseEnv)) {
    if (/^HERDR_/i.test(k) || HOST_HARNESS_ENV.test(k)) removed.push(k);
    else env[k] = v;
  }
  env.HERDR_CONFIG_PATH = configPath;
  return { env, delta: { added: { HERDR_CONFIG_PATH: configPath }, removed: removed.sort() } };
}

// herdr's own config for the run: no release checks and no runtime agent-manifest
// downloads, so the pinned binary also pins detection behavior (K1 §2 caveat).
export const HERDR_RUN_CONFIG = '[update]\nversion_check = false\nmanifest_check = false\n';

// Compare a pane's environment (as reported by env-probe.mjs) with the env the herdr
// server was launched with. Names only, plus the probe's allowlisted values.
export function paneEnvDelta(serverEnvNames, probe) {
  const server = new Set(serverEnvNames);
  const pane = new Set(probe.names);
  return {
    added: [...pane].filter((n) => !server.has(n)).sort(),
    removed: [...server].filter((n) => !pane.has(n)).sort(),
    values: probe.values,
    note: 'Names compared against the herdr server launch env. Values recorded for HERDR_*, TERM and COLORTERM only. Additions can also come from the pane shell\'s own startup files.',
  };
}
