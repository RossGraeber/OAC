// Run-manifest helpers: driver commit, OS, harness versions, harness config hashes, and the
// launch-environment delta. Nothing here writes outside the run's own directories.
//
// Harness config is READ AND HASHED ONLY (sha256), never copied, printed, or written:
// ADR-001 boundaries 3/13 and oac-boundaries check 10. The config paths are spelled out
// once, in harnessConfigFiles(); in the driver they are handed only to the hash reader in
// hashHarnessConfig(). (The self-test also calls harnessConfigFiles() to plant synthetic
// files in its own temp dirs, behind a path guard -- see test/selftest.mjs makeBase().)
//
// Environment VALUES are never read here except for the allowlist in env-probe.mjs
// (HERDR_*, TERM, COLORTERM). Everything else is compared by variable NAME only, so a
// provider credential that happens to sit in the operator's environment is never read.

import { createHash } from 'node:crypto';
import { readFileSync, existsSync } from 'node:fs';
import { homedir, platform, release, arch, type } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { runBounded } from './proc.mjs';

export const MANIFEST_SCHEMA_VERSION = 1;

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

// `<harness> --version` for each harness the scenario launches. Bounded; a missing binary
// is recorded as N/A, never guessed.
export async function harnessVersions(harnesses, { env = process.env, deadlineMs = 15000 } = {}) {
  const out = {};
  for (const h of harnesses) {
    const [file, args] = process.platform === 'win32' ? ['cmd.exe', ['/d', '/s', '/c', h, '--version']] : [h, ['--version']];
    const r = await runBounded(file, args, { deadlineMs, env });
    if (r.spawnError || r.exitCode !== 0) out[h] = `N/A (${r.spawnError ? `not runnable: ${r.spawnError}` : r.timedOut ? 'timed out' : `exit ${r.exitCode}`})`;
    else out[h] = r.stdout.trim().split('\n')[0];
  }
  return out;
}

// An enclosing Claude Code session's own variables (#163). When the driver is started from a
// shell inside a Claude Code session, these would reach the harness under test through the
// herdr server and pane: it would run as that session's child (no transcript, its permission
// mode) and could reach models through the enclosing app's relay (ANTHROPIC_BASE_URL) instead
// of its own sign-in (README "Reuse contract" point 6). Other ANTHROPIC_* variables, e.g. an
// operator's own ANTHROPIC_API_KEY, are the operator's sign-in and are kept (operator decision
// on #163).
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
