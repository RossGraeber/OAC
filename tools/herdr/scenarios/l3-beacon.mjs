// l3-beacon: the Beacon live leg (L3, #168) through the herdr driver (L3b, #190).
//
// NOT A GATE. L3 is not a gate (docs/planning/PINS.md Beacon row: `Gates affected: none`). The
// run outcome (PASS / FAIL / NOT RUN) says only whether a phase ran as specified; it is never a
// step verdict (oac-gates references/scripted-runs.md "Three vocabularies"). The §13 draft is
// L3c's (tools/herdr/lib/l3-report.mjs, #191); this scenario records, it does not judge.
//
// LIVE STATUS: UNVERIFIED. Exercised only against the test doubles in tools/herdr/test/
// (fake-herdr, fake-claude, fake-codex, fake-beacon); it has never driven a real herdr, Claude
// Code, Codex or Beacon. Pane-text patterns are the unconfirmed ones in lib/g1.mjs and lib/g2.mjs.
//
// What it runs: docs/planning/decisions/L1-beacon-memory.md §12, except the operator steps.
// Three phases, selected by --param phase, because the operator's steps happen between them:
//   baseline (B0)                 hashes, versions, env names; declares the ONE 60-minute L3 box
//   probe (B1 record, B2, B3, B4) refuses (NOT RUN) once that box has expired
//   verify (B7 hash check)        after the operator's own B7 teardown and restore
// B1 is recorded NOT RUN (operator decision 2026-09-30 on #168: Beacon was installed before any
// baseline; no uninstall/reinstall). Its evidence is the 2026-09-30 MSI findings in L1 §12 plus
// the weaker `.beacon.bak` evidence (the MSI's second pass can overwrite those backups; the
// driver does not read them). B5 and B6 are NOT RUN: they are Beacon steps only the operator
// may do. The Codex daemon's state before the probe is always recorded as a finding.
//
// Operator and agent commands (herdr at the PINS.md pin; Claude Code and Codex signed in the
// way the operator normally uses them; Beacon 1.3.29 installed by the operator; the operator
// stops any running Codex app-server daemon before the probe). On Windows Beacon is not on
// PATH, so --param beaconBin takes its absolute path (e.g. C:\Program Files\Beacon\bin\beacon.exe):
//
//   # agent: B0 (declares the 60-minute box; launches no harness)
//   node tools/herdr/run.mjs --scenario l3-beacon --param phase=baseline \
//     --param beaconBin=<abs path to beacon> --out <baseline dir>
//   # agent: B1 record, B2-B4 (Claude Code's workspace-trust, project-MCP-server and
//   # development-channels dialogs are accepted by the driver: accept=driver is the default
//   # since #196, each dialog read verbatim first and its accept recorded as `driver`;
//   # --param accept=human makes the driver send nothing and wait for the operator)
//   node tools/herdr/run.mjs --scenario l3-beacon --param phase=probe \
//     --param baselineRun=<baseline dir> --param beaconBin=<abs path to beacon> \
//     --out <probe dir>
//   # operator: stop the Codex app-server daemon the probe started or found running
//   # (`codex app-server daemon stop`; it has Beacon's [otel] config loaded and would keep
//   # exporting to Beacon), THEN the L1 §12 B7 teardown and restore of the B0 backups (outside
//   # the driver)
//   # agent: B7 hash check (launches no harness, runs no Beacon command)
//   node tools/herdr/run.mjs --scenario l3-beacon --param phase=verify \
//     --param baselineRun=<baseline dir> --out <verify dir>
// With --param beaconCli=off the driver runs no Beacon command at all, and the operator supplies
// --param beaconVersion=<`beacon version` output> and --param beaconLog=<runtime.jsonl path>;
// both are recorded as operator-supplied, and the poll path (sync --print) is NOT RUN.
//
// Beacon CLI: READ-ONLY ALLOWLIST (operator decision 2026-09-30 on #168). With the default
// --param beaconCli=readonly the driver runs exactly these, bounded, output parsed in-process
// and never echoed (BEACON_ALLOWLIST below; anything else throws before a process starts):
//   beacon version                     must report 1.3.29 (L1 §2), else NOT RUN; re-checked at
//                                      the end of the probe, a change is NOT RUN
//   beacon endpoint status --system    the runtime-log path and the `Beacon Managed:` line
//   beacon endpoint claude sync --print  poll-path hit counts (B2, B3)
//   beacon endpoint codex sync --print   poll-path hit counts (B4)
// `sync --print` writes nothing, confirmed from source at the pinned tag (all at
// https://github.com/Asymptote-Labs/agent-beacon/blob/v1.3.29/<path>, read 2026-09-30):
//   - cli/beacon/cmd/endpoint_claude.go L77 ("Print mapped events as JSON without writing them
//     or advancing the cursor (dry run)"), L92-L103: with --print, Write is false and neither
//     StatePath nor LogPath is set; endpoint_codex.go L67 and L74-L84 do the same for Codex;
//   - cli/beacon/internal/claudesession/collect.go L74-L78 (LoadState reads nothing for an
//     empty path), L97-L100 (State.Save returns before any write for an empty path), L151-L154
//     (no retention guard under Print), L270-L280 (emit under Print only encodes to stdout;
//     writer.AppendEvent is never reached); codexsession/collect.go L80-L83 and L197-L207 the
//     same; claudesession/store.go and codexsession/store.go only os.Open their inputs;
//   - no PersistentPreRun on the root or endpoint command (cli/beacon/cmd/root.go,
//     cli/beacon/cmd/endpoint.go), so no hook runs before the subcommand.
// With no state file, `sync --print` re-emits EVERY session from the start, oldest first
// (claudesession/store.go List sorts by mtime ascending), so the probe's session comes LAST. Its
// stdout is therefore scanned line by line as it streams, with no size cap and nothing kept but
// the lines holding a probe value; a sync that times out or fails is recorded NOT RUN for the
// poll path only (beacon.sync.<step>) and the probe goes on.
// `beacon version` writes nothing (cli/beacon/cmd/version.go L19-L25: one Fprintln; the version
// check runs only with --check). `beacon endpoint status --system` writes nothing either
// (source audit at v1.3.29, PR #195 review), but it is not side-effect free, and the record
// says so (beacon.status.sideEffects): it runs `<name> --version` (2 s timeout) for each of about
// 25 harness binaries it finds on PATH, including claude and codex (internal/harness/harness.go
// detectExecutable/commandVersion, DiscoverAll); it reads harness config files (Beacon's reads,
// no credential files); it makes loopback TCP/HTTP probes to ports 4317, 4318 and 13133
// (endpoint/collector/collector.go); it makes an outbound HTTPS GET to Asymptote's ingest only
// if the system install is enrolled (endpoint/asymptote/status.go; the `Beacon Managed: not
// connected` finding tells the operator whether that could happen); and on Linux with systemd
// it runs `systemctl is-enabled` / `is-active` queries. The driver never
// installs, uninstalls, connects or configures Beacon, never runs a memory or hooks subcommand,
// and never runs herdr's hook-writing command. OAC product code never calls Beacon (L1 §1).
//
// Probe values (L1 §12 "Probe content"): one marker and one fake secret-shaped token per
// delivery path, generated in-process (lib/l3.mjs makeProbeMarkers), registered with
// ctx.redactLiteral so every capture and the run manifest carry placeholders only, and searched
// for by FULL value, never by prefix. They reach the harnesses only through the staged channel
// server (B2) and the staged app-server client (B4); herdr types no probe value, and every typed
// text is checked (assertNoSpoof, assertNoMarkerLeak). The "scratch copy" of gate-servers/ is
// this run's own scratch directory under os.tmpdir(); only that copy's g5-cases.json is
// augmented; the committed tools/herdr/gate-servers/ files are never edited.
//
// Files: the driver reads and hashes the four harness-config files at Beacon's fixed $HOME
// paths (plus the CLAUDE_CONFIG_DIR / CODEX_HOME equivalents when set), reads Beacon's runtime
// log (and rotated siblings) read-only, and, with --param readSessionFile=true (operator
// decision 2026-09-30: allowed, entry types and flags only), reads the scratch probe project's
// own Claude session file(s) and records, for entries holding the probe marker, the entry
// `type`, `isMeta`, and the attachment `type` and key names, never content. It writes no
// harness config (oac-boundaries check 10), reads no credential file and asks no OS credential
// store for anything; the harnesses serve every model turn from their own sign-in.

import { existsSync, readFileSync, readdirSync, realpathSync, statSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { spawn } from 'node:child_process';
import { basename, dirname, isAbsolute, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { NotRunError, DriverError } from '../lib/herdr.mjs';
import { parseClaudeVersions, pinsReadWarning, parseClaudeCliVersion, claudeVersionWarning, parseCodexVersions, parseCodexCliVersion, codexVersionWarning, parseCodexDaemonVersion, CLAUDE_PIN_ROW, CODEX_PIN_ROW, CODEX_DAEMON_VERSION_FIELDS } from '../lib/pins.mjs';
import { harnessVersions } from '../lib/manifest.mjs';
import { runBounded, descendants, killTree } from '../lib/proc.mjs';
import { CODEX_DAEMON_SCRATCH_HOLDER } from '../lib/scratch.mjs';
import { committedFile, classifyScreen, driverMayAcceptExpecting, DIALOG_KINDS } from '../lib/g1.mjs';
import { G2_LAUNCH, waitCodexReady, loadedSince, codexReadyTimeoutFinding, multipleNewThreadsFinding, classifyCodexScreen, driverMayAcceptCodex, CODEX_DIALOG_KINDS, paneArgv, identifyTuiThread, sanitizeTranscript } from '../lib/g2.mjs';
import { makeAgent, stopper, stageGateFiles, GATE_SERVERS_DIR } from '../lib/gate-common.mjs';
import { G5_LAUNCH, G5_SERVER_FILES, G5_CLIENT_FILES, PINS_PATH, assertNoSpoof, parseJsonl, g5ClaudeFacts, g5CodexFacts, threadIdleOnWire } from '../lib/g5.mjs';
import {
  L3_RECORD_VERSION, makeProbeMarkers, markerRecords, augmentCaseTable, scanRuntimeLog, redactedExcerpt, harnessConfigTargets, hashConfig, compareSections,
  assertNoMarkerLeak, findMarkerLeaks, sha256, L3_CLAUDE_CASE, L3_CODEX_CASE, PLACEHOLDER_RE,
} from '../lib/l3.mjs';
import { createRedactor } from '../lib/redact.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..', '..');
const sleep = (ms) => new Promise((r) => setTimeout(r, Math.max(0, ms)));
const cap = (s, n) => (String(s ?? '').length > n ? `${String(s).slice(0, n)}… (${String(s).length} chars)` : String(s ?? ''));

export const BEACON_VERSION = '1.3.29';
export const L3_BOX_MS = 60 * 60 * 1000; // L1 §12 "Timebox: 60 minutes", one box across all phases
export const PHASES = Object.freeze(['baseline', 'probe', 'verify']);
// The tool-invocation actions L1 §12 B3 names (as lib/l3-report.mjs TOOL_INVOKED_ACTIONS).
export const TOOL_INVOKED_ACTIONS = Object.freeze(['mcp.tool_invoked', 'tool.invoked']);

// The ONLY Beacon argv the driver may run (operator decision 2026-09-30 on #168).
export const BEACON_ALLOWLIST = Object.freeze({
  version: Object.freeze(['version']),
  status: Object.freeze(['endpoint', 'status', '--system']),
  claudeSync: Object.freeze(['endpoint', 'claude', 'sync', '--print']),
  codexSync: Object.freeze(['endpoint', 'codex', 'sync', '--print']),
});
// --param beaconBin must be the executable itself: never a script run under an interpreter.
export const BEACON_BIN_NAME = /^beacon(?:\.exe)?$/i;
export const STATUS_SIDE_EFFECTS = 'no writes; spawns `<harness> --version` (2 s timeout) for each of about 25 harness binaries found on PATH; reads harness config files (not credentials); loopback probes to 4317/4318/13133; an outbound HTTPS GET to Asymptote ingest only if the system install is enrolled; `systemctl is-enabled`/`is-active` on Linux with systemd (source audit, v1.3.29)';

/**
 * Run one command and scan its stdout line by line AS IT STREAMS: no size cap, and nothing is
 * kept but the lines holding one of `needles` (in-process only; never recorded). Bounded by
 * deadlineMs and abortSignal; no shell.
 * @returns {Promise<{ exitCode, signal, timedOut, aborted, spawnError, lines, bytes, stderrBytes, kept: string[], startedAt, endedAt }>}
 */
export function streamScan(file, args, { deadlineMs, env, abortSignal, needles = [] }) {
  if (!(deadlineMs > 0)) throw new Error('streamScan: an explicit deadlineMs > 0 is required');
  const startedAt = new Date().toISOString();
  return new Promise((done) => {
    const res = { exitCode: null, signal: null, timedOut: false, aborted: false, spawnError: null, lines: 0, bytes: 0, stderrBytes: 0, kept: [], startedAt, endedAt: null };
    let settled = false;
    let child;
    let rest = '';
    let timer = null;
    let hard = null;
    const finish = () => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      clearTimeout(hard);
      abortSignal?.removeEventListener('abort', onAbort);
      if (rest) take(rest);
      rest = '';
      res.endedAt = new Date().toISOString();
      done(res);
    };
    const take = (line) => {
      if (!line.trim()) return;
      res.lines += 1;
      if (needles.some((n) => line.includes(n))) res.kept.push(line);
    };
    const kill = () => {
      killTree(child?.pid);
      hard = setTimeout(finish, 3000);
    };
    const onAbort = () => {
      res.aborted = true;
      kill();
    };
    try {
      child = spawn(file, args, { env, stdio: ['ignore', 'pipe', 'pipe'], detached: process.platform !== 'win32', windowsHide: true });
    } catch (err) {
      res.spawnError = err.code || String(err);
      finish();
      return;
    }
    timer = setTimeout(() => {
      res.timedOut = true;
      kill();
    }, deadlineMs);
    if (abortSignal) {
      if (abortSignal.aborted) queueMicrotask(onAbort);
      else abortSignal.addEventListener('abort', onAbort, { once: true });
    }
    child.stdout.setEncoding('utf8');
    child.stdout.on('data', (d) => {
      res.bytes += Buffer.byteLength(d);
      const parts = (rest + d).split('\n');
      rest = parts.pop();
      for (const p of parts) take(p);
    });
    child.stderr.on('data', (d) => {
      res.stderrBytes += d.length;
    });
    child.on('error', (err) => {
      res.spawnError = err.code || String(err);
      finish();
    });
    child.on('close', (code, sig) => {
      res.exitCode = code;
      res.signal = sig;
      finish();
    });
  });
}

export function beaconArgv(key) {
  if (typeof key !== 'string' || !Object.hasOwn(BEACON_ALLOWLIST, key)) throw new DriverError(`beacon command ${JSON.stringify(key)} is not on the read-only allowlist (${Object.keys(BEACON_ALLOWLIST).join(', ')})`);
  return [...BEACON_ALLOWLIST[key]];
}

// L1 §12 live observations: the Windows MSI's system-mode log, else the per-user one.
export const defaultBeaconLog = (platform = process.platform, home = homedir()) =>
  platform === 'win32' ? 'C:\\ProgramData\\Beacon\\Endpoint\\logs\\runtime.jsonl' : join(home, '.beacon', 'endpoint', 'logs', 'runtime.jsonl');

// `beacon version` prints "beacon version <full version>" (cli/beacon/cmd/version.go@v1.3.29).
export function parseBeaconVersion(stdout) {
  const m = /^beacon version v?(\d+\.\d+\.\d+)(?![\d.])/m.exec(String(stdout ?? ''));
  return m ? m[1] : null;
}

// `beacon endpoint status` text (cli/beacon/cmd/endpoint_install.go@v1.3.29 L206-L245): only the
// lines L3 needs. The Managed line is printed at endpoint_install.go@v1.3.29 L242 by
// managedIngestStatusLine() (cli/beacon/cmd/endpoint_connect.go@v1.3.29 L286-L315, read
// 2026-10-01).
export function parseBeaconStatus(stdout) {
  const s = String(stdout ?? '');
  const line = (re) => re.exec(s)?.[1]?.trim() ?? null;
  return { agentVersion: line(/^Beacon Endpoint Agent\s+(.+)$/m), logPath: line(/^Runtime log:\s*(.+)$/m), managed: line(/^Beacon Managed:\s*(.+)$/m), service: line(/^Service:\s*(.+)$/m) };
}

// Is the parsed `Beacon Managed:` value Local mode (no hosted forwarding)? #209. At v1.3.29
// managedIngestStatusLine() (cli/beacon/cmd/endpoint_connect.go@v1.3.29) prints, when
// ManagedIngest.Enabled is false, exactly one of:
//   L289  "Beacon Managed: not connected (" + status.Message + ")"
//   L291  "Beacon Managed: not connected (run `beacon endpoint connect`)"   <- the live line,
//                                                                              2026-10-01
// and, when Enabled is true, "Beacon Managed: connected ..." (L294 onward). Enabled=false does
// NOT always mean forwarding is off (PR #215 review), so "not connected (<anything>)" is not
// accepted. The L289 Message comes from Status() (cli/beacon/internal/endpoint/asymptote/
// status.go@v1.3.29); only these exact values are Local mode (BEACON_MANAGED_LOCAL_VALUES):
//   - bare `not connected`: the L1 §12 2026-09-30 record's shape
//   - L291 hint: not enrolled, no connect pending (status.go L57-L60 with ErrNotEnrolled)
// Never accepted (each stays a finding):
//   - L62 err.Error() text: the enrollment record exists but could not be read (e.g. Access is
//     denied unelevated, or invalid JSON), so forwarding may be on;
//   - L64 and L71 "connect incomplete": a first connect (cli/beacon/internal/endpoint/asymptote/
//     connect.go@v1.3.29) writes the unit and starts the forwarder (L261-L267) before
//     SaveEnrollment (L290) and clearConnectPending (L302); only the deferred cleanup (L108-L119)
//     stops it, and only on an error return, so a Ctrl-C (L199-L200) or a failed unload leaves
//     the L64 status while a forwarder with a valid key runs; likewise L71 between L290 and
//     L302 (its "partial forwarder was stopped" is a fixed string nothing checks). Either also
//     means `beacon endpoint connect` was run, contrary to L1 §4 Q3;
//   - L73 (pending state unreadable); L78 "re-connect incomplete" (the previous connection's
//     forwarder may still run); L101 "disconnected; credentials for device <id> kept" (the
//     device id's shape is not pinned here);
//   - `connected ...`; a missing line (null); any other wording.
// Exact string match, never a prefix or pattern match (fail safe).
export const BEACON_MANAGED_LOCAL_VALUES = Object.freeze([
  'not connected',
  'not connected (run `beacon endpoint connect`)', // endpoint_connect.go@v1.3.29 L291; live 2026-10-01
]);
export const beaconManagedIsLocal = (value) => typeof value === 'string' && BEACON_MANAGED_LOCAL_VALUES.includes(value);

// The one L3 box, declared by the baseline phase.
export function boxState(start, now = Date.now(), budgetMs = L3_BOX_MS) {
  const t = Date.parse(start);
  if (typeof start !== 'string' || !Number.isFinite(t)) return { valid: false, start: start ?? null, budgetMs };
  const end = t + budgetMs;
  return { valid: true, start, budgetMs, end: new Date(end).toISOString(), remainingMs: end - now, expired: now >= end };
}

// Claude Code's project directory name for a working directory: every character outside
// [A-Za-z0-9] becomes '-'. Observed on this machine's own ~/.claude/projects naming; UNVERIFIED
// as a documented rule, so both the path and its realpath are tried.
export const claudeProjectSlug = (dir) => String(dir).replace(/[^A-Za-z0-9]/g, '-');

const ENUM = /^[A-Za-z][A-Za-z0-9_.-]{0,47}$/;
const enumOrNull = (v, markers) => (typeof v === 'string' && ENUM.test(v) && !findMarkerLeaks(v, markers).length ? v : v == null ? null : '<non-enum>');

// Entry shapes, never content, for session-file lines holding the claude-channel probe values.
export function sessionEntryShapes(text, markers) {
  const m = markers.find((x) => x.id === 'claude-channel');
  const entries = [];
  let lines = 0;
  for (const raw of String(text ?? '').split('\n')) {
    if (!raw.trim()) continue;
    lines += 1;
    const marker = raw.includes(m.marker);
    const token = raw.includes(m.token);
    if (!marker && !token) continue;
    let o = null;
    try {
      o = JSON.parse(raw);
    } catch {
      /* recorded as unparsed */
    }
    if (!o || typeof o !== 'object' || Array.isArray(o)) {
      entries.push({ parsed: false, markerPresent: marker, tokenPresent: token });
      continue;
    }
    const att = o.attachment && typeof o.attachment === 'object' && !Array.isArray(o.attachment) ? o.attachment : null;
    entries.push({
      parsed: true,
      type: enumOrNull(o.type, markers),
      isMeta: typeof o.isMeta === 'boolean' ? o.isMeta : null,
      attachmentType: att ? enumOrNull(att.type, markers) : null,
      attachmentKeys: att ? Object.keys(att).filter((k) => ENUM.test(k) && !findMarkerLeaks(k, markers).length).sort() : null,
      markerPresent: marker,
      tokenPresent: token,
    });
  }
  return { lines, entries };
}

// Per-delivery-path COUNTS from scanRuntimeLog() hits (L3c contract, #191): lines holding the
// marker, lines holding the fake token verbatim, and line counts per event.action, per JSON
// path, per collection method and per harness. Value-free (paths are already substituted).
export function hitCounts(hits, markers) {
  const out = {};
  const bump = (o, k) => {
    o[k] = (o[k] ?? 0) + 1;
  };
  for (const m of markers) {
    const hs = hits.filter((h) => h.markerId === m.id);
    const c = { lines: hs.length, markerLines: hs.filter((h) => h.markerPresent).length, tokenLines: hs.filter((h) => h.tokenVerbatim).length, byAction: {}, byPath: {}, byCollectionMethod: {}, byHarness: {} };
    for (const h of hs) {
      bump(c.byAction, h.eventAction ?? '<none>');
      for (const p of new Set([...h.markerPaths, ...h.tokenPaths])) bump(c.byPath, p);
      bump(c.byCollectionMethod, h.harness.collection_method ?? '<none>');
      bump(c.byHarness, h.harness.name ?? '<none>');
    }
    out[m.id] = c;
  }
  return out;
}

// The probe's own session ids per harness. null (unknown), never [], when none was found: the
// Claude slug rule is UNVERIFIED, and an empty list would make every Claude hit look "outside
// the probe session".
export function probeSessionIds(sessionFile, threadId) {
  return {
    claude: sessionFile?.read && sessionFile.sessionIds?.length ? [...sessionFile.sessionIds].sort() : null,
    codex: threadId ? [threadId] : null,
    note: 'harness-side ids (Claude session file names, Codex thread id); their equality with Beacon session.id is UNVERIFIED; null means unknown',
  };
}

// Excerpt placeholders read as a secret assignment to run.mjs's manifest redaction; the manifest
// stores them in a form it leaves alone: {L3-MARKER <id>} / {L3-FAKE-TOKEN <id>}.
export const manifestSafePlaceholders = (text) => String(text).replace(new RegExp(PLACEHOLDER_RE.source, 'g'), (p) => `{${p.slice(1, -1).replace(':', ' ')}}`);

// Operator texts typed by herdr: never a probe value, a G5 spoofing body, or a frame marker.
export const DEFAULT_REPLY_PROMPT = 'Use the g5spike reply tool to send back the probe code from the most recent channel message, exactly as it appears there, and nothing else.';
export const DEFAULT_THREAD_MARKER = 'L3 thread marker: say hi in one word.';

export function loadBaseline(runDir) {
  if (!runDir) throw new DriverError('--param baselineRun=<the baseline phase\'s --out directory> is required for this phase');
  const p = join(resolve(runDir), 'run-manifest.json');
  if (!existsSync(p)) throw new DriverError(`no run-manifest.json in --param baselineRun (${runDir})`);
  let m;
  try {
    m = JSON.parse(readFileSync(p, 'utf8'));
  } catch {
    throw new DriverError('the baseline run-manifest.json is not JSON');
  }
  const b = m?.scenarioData?.l3;
  if (m?.scenario?.name !== 'l3-beacon' || b?.phase !== 'baseline') throw new DriverError('--param baselineRun is not an l3-beacon baseline run');
  if (b.version !== L3_RECORD_VERSION) throw new DriverError(`the baseline record is version ${JSON.stringify(b.version)}; this scenario writes version ${L3_RECORD_VERSION}`);
  if (m.outcome !== 'PASS') throw new NotRunError(`the baseline run ${m.runId} ended ${m.outcome} (${m.outcomeReason ?? 'no reason'}); there is no B0 to compare against, so this phase does not run`);
  if (!Array.isArray(b.config?.hashes) || !b.box?.start) throw new DriverError('the baseline record has no config hashes or no box start');
  return { runId: m.runId, box: b.box, hashes: b.config.hashes, versions: b.versions };
}

export default {
  name: 'l3-beacon',
  description: 'Beacon live leg (L3, #168): baseline (B0) / probe (B1 record, B2-B4) / verify (B7 hash check). Not a gate; records only.',
  harnesses: ['claude', 'codex'],
  defaults: {
    launch: [...G5_LAUNCH],
    timeboxMs: L3_BOX_MS,
    params: {
      phase: '',
      baselineRun: '',
      beaconCli: 'readonly',
      beaconBin: '',
      beaconVersion: '',
      beaconLog: '',
      readSessionFile: 'true',
      accept: 'driver', // #196; accept=human remains available
      replyPrompt: DEFAULT_REPLY_PROMPT,
      threadMarker: DEFAULT_THREAD_MARKER,
      beaconSettleMs: '20000',
      beaconTimeoutMs: '120000',
      beaconSyncTimeoutMs: '300000',
      startupTimeoutMs: '120000',
      humanAcceptTimeoutMs: '300000',
      handshakeTimeoutMs: '90000',
      cliTimeoutMs: '60000',
      clientTimeoutMs: '60000',
      wireTimeoutMs: '15000',
      readyTimeoutMs: '120000', // #204: bound on the verified-ready wait before the thread marker
      attachTimeoutMs: '180000',
      listPollMs: '3000',
      turnTimeoutMs: '300000',
      settleMs: '3000',
      pollMs: '1000',
      busyIndicator: 'esc to interrupt',
      readLines: '2000',
      maxDialogs: '12',
    },
  },

  async run(ctx) {
    const { herdr, params } = ctx;
    const num = (k) => {
      const v = Number(params[k]);
      if (!(v >= 0)) throw new DriverError(`--param ${k} must be a non-negative number`);
      return v;
    };
    const phase = params.phase;
    if (!PHASES.includes(phase)) throw new DriverError(`--param phase must be one of ${PHASES.join(', ')}`);
    const cliMode = params.beaconCli;
    if (!['readonly', 'off'].includes(cliMode)) throw new DriverError('--param beaconCli must be readonly or off');
    const accept = params.accept;
    // #196 (docs/planning/decisions/K-196-driver-accepts-dialogs.md): in dev/test runs the
    // driver accepts Claude Code's workspace-trust, project-MCP-server and development-channels
    // dialogs itself by default; each is read verbatim first and its accept recorded as `driver`.
    // accept=human remains available (the driver then sends nothing to any dialog).
    if (!['driver', 'human'].includes(accept)) throw new DriverError('--param accept must be driver or human');

    const l3 = {
      version: L3_RECORD_VERSION,
      phase,
      box: null,
      versions: { beacon: null, beaconSource: null, claudeCli: null, codex: { cli: null, daemon: null, wire: null }, pins: null },
      config: { hashes: null, compare: null, findings: [], envSet: null, trailingComment: null },
      markers: [],
      scan: null,
      findings: [],
      steps: {},
      beacon: { cli: cliMode, bin: cliMode === 'readonly' ? params.beaconBin || null : null, calls: [], status: null, log: null, sync: {} },
      acceptPolicy: accept,
      probeSessions: null,
      dialogs: [],
      herdrStates: [],
      stoppedAt: null,
    };
    const finding = (t) => {
      l3.findings.push(t);
      ctx.finding(t);
    };
    const stop = stopper(herdr, l3);
    const abortSignal = herdr.abortSignal;
    const aborted = () => {
      if (abortSignal?.aborted) throw new NotRunError(herdr.abortReason());
    };
    let markers = [];
    let remaining = () => ctx.remainingMs();

    // --- Beacon: the read-only allowlist, nothing else ------------------------------------------
    // The verify phase runs no Beacon command (Beacon may already be uninstalled by then).
    let beaconBin = null;
    if (phase === 'verify') {
      /* no Beacon command in this phase */
    } else if (cliMode === 'readonly') {
      if (!params.beaconBin || !isAbsolute(params.beaconBin)) throw new DriverError('--param beaconBin=<absolute path to the beacon executable> is required with beaconCli=readonly (on Windows Beacon is not on PATH)');
      if (!BEACON_BIN_NAME.test(basename(params.beaconBin))) throw new DriverError('--param beaconBin must name the beacon executable itself (basename `beacon` or `beacon.exe`)');
      if (!existsSync(params.beaconBin)) throw new DriverError('--param beaconBin does not exist');
      beaconBin = params.beaconBin;
    } else if (!params.beaconVersion || !params.beaconLog) {
      throw new DriverError('with beaconCli=off the operator supplies --param beaconVersion and --param beaconLog');
    }
    // The only way a Beacon process is started: the executable itself, an allowlisted argv, no
    // shell, no interpreter.
    const beaconCommand = (key) => {
      if (!beaconBin) throw new DriverError('internal: no Beacon command may run with beaconCli=off');
      return [beaconBin, beaconArgv(key)];
    };
    const beacon = async (key, step) => {
      const [file, args] = beaconCommand(key);
      const argv = args;
      aborted();
      const r = await runBounded(file, args, { deadlineMs: Math.min(num('beaconTimeoutMs'), Math.max(1, remaining())), env: process.env, abortSignal });
      const rec = { key, argv: ['beacon', ...argv], step, startedAt: r.startedAt, endedAt: r.endedAt, exitCode: r.exitCode, timedOut: r.timedOut, spawnError: r.spawnError, stdoutBytes: Buffer.byteLength(r.stdout ?? ''), stderrBytes: Buffer.byteLength(r.stderr ?? '') };
      l3.beacon.calls.push(rec);
      aborted();
      if (r.timedOut) stop(`\`beacon ${argv.join(' ')}\` timed out; nothing re-run`);
      return { r, rec };
    };
    const beaconVersionNow = async (step) => {
      if (cliMode === 'off') return { version: /^\s*(?:beacon version\s+)?v?(\d+\.\d+\.\d+)\s*$/.exec(params.beaconVersion)?.[1] ?? null, source: 'operator-supplied (--param beaconVersion; beaconCli=off)' };
      const { r } = await beacon('version', step);
      return { version: r.exitCode === 0 && !r.spawnError ? parseBeaconVersion(r.stdout) : null, source: '`beacon version` (read-only allowlist)' };
    };
    const requireBeaconVersion = async (step) => {
      const { version, source } = await beaconVersionNow(step);
      l3.versions.beacon = version;
      l3.versions.beaconSource = source;
      if (version !== BEACON_VERSION) stop(`Beacon reports ${version ? `version ${version}` : 'no readable version'} (${source}); L3 runs only against Beacon ${BEACON_VERSION} (L1 §2). Nothing launched`);
      return version;
    };
    let beaconLog = params.beaconLog || null;
    const beaconStatus = async (step) => {
      if (cliMode === 'off') {
        l3.beacon.status = { source: 'NOT RUN (beaconCli=off)' };
        l3.beacon.log = { path: beaconLog, source: 'operator-supplied (--param beaconLog; beaconCli=off)' };
        return;
      }
      const { r } = await beacon('status', step);
      const s = parseBeaconStatus(r.stdout);
      l3.beacon.status = { step, exitCode: r.exitCode, logPath: s.logPath, managed: s.managed, service: s.service, agentVersion: s.agentVersion, sideEffects: STATUS_SIDE_EFFECTS };
      if (!beaconLog) beaconLog = defaultBeaconLog();
      l3.beacon.log = { path: beaconLog, source: params.beaconLog ? '--param beaconLog' : 'default (L1 §12 live observations)' };
      if (s.logPath && resolve(s.logPath) !== resolve(beaconLog)) finding(`\`beacon endpoint status --system\` reports a runtime log path other than the one read (${s.logPath} vs ${beaconLog}); pass --param beaconLog to read the reported one`);
      if (!beaconManagedIsLocal(s.managed)) finding(`\`beacon endpoint status --system\` reports "Beacon Managed: ${s.managed ?? '(line not found)'}", not a recognised Local-mode value (BEACON_MANAGED_LOCAL_VALUES); hosted forwarding may be on (L1 §4 Q3: stay in Local mode)`);
    };

    // --- versions (drift is a VERSION WARNING finding, never a stop: versions float, #216) -------
    const pinsAndCli = () => {
      const pinsFile = committedFile(REPO, PINS_PATH);
      const text = pinsFile.bytes.toString('utf8');
      const cv = parseClaudeVersions(text);
      const xv = parseCodexVersions(text);
      const cRaw = ctx.harnessVersion('claude');
      const xRaw = ctx.harnessVersion('codex');
      const cli = { claude: parseClaudeCliVersion(cRaw), codex: parseCodexCliVersion(xRaw) };
      l3.versions.claudeCli = cli.claude;
      l3.versions.codex.cli = cli.codex;
      l3.versions.cliOutput = { claude: cRaw, codex: xRaw };
      l3.versions.pins = {
        claude: { row: CLAUDE_PIN_ROW, minimum: cv.minimum, lastTested: cv.lastTested, differs: cli.claude !== cv.lastTested },
        codex: { row: CODEX_PIN_ROW, minimum: xv.minimum, lastTested: xv.lastTested, differs: cli.codex !== xv.lastTested },
        headCommit: pinsFile.headCommit,
        workingTreeMatchesHead: pinsFile.workingTreeMatchesHead,
      };
      if (!pinsFile.workingTreeMatchesHead) finding(`${PINS_PATH} has uncommitted changes; the versions recorded are the committed ones`);
      for (const w of [pinsReadWarning(cv, 'L3'), pinsReadWarning(xv, 'L3')]) if (w) finding(w);
      for (const w of [
        claudeVersionWarning({ observed: cli.claude, lastTested: cv.lastTested, minimum: cv.minimum, source: '`claude --version`', gate: 'L3' }),
        codexVersionWarning({ observed: cli.codex, lastTested: xv.lastTested, minimum: xv.minimum, source: '`codex --version`', gate: 'L3' }),
      ]) if (w) finding(w);
      return { cli, cv, xv };
    };
    const configNow = () => {
      const { targets, findings } = harnessConfigTargets();
      const hashes = hashConfig(targets);
      return { hashes, findings };
    };

    // --- phase: baseline (B0) -----------------------------------------------------------------------
    const baseline = async () => {
      // The box is declared before B0, and spans every phase (L1 §12 "Timebox").
      l3.box = { start: new Date().toISOString(), budgetMs: L3_BOX_MS, sourceRunId: null };
      await requireBeaconVersion('B0');
      pinsAndCli();
      await beaconStatus('B0');
      const c = configNow();
      l3.config.hashes = c.hashes;
      l3.config.envSet = { CLAUDE_CONFIG_DIR: !!process.env.CLAUDE_CONFIG_DIR, CODEX_HOME: !!process.env.CODEX_HOME };
      for (const f of c.findings) finding(f);
      l3.config.trailingComment = c.hashes.filter((h) => h.kind === 'codex-config').map((h) => ({ label: h.label, present: h.present, headers: h.trailingComment?.headers ?? [], anyAfterOtel: h.trailingComment?.anyAfterOtel ?? false }));
      for (const t of l3.config.trailingComment) if (t.anyAfterOtel) finding(`${t.label} has a table header with a trailing comment after an [otel] section: Beacon's [otel] merge drops such a table (L1 §11 item 4 edge case)`);
      l3.steps.B0 = { status: 'recorded', note: 'hashes, versions and env-variable names only; the operator keeps the file backups outside the repository (L1 §12 B0)' };
    };

    // --- phase: verify (B7 hash check) -----------------------------------------------------------------
    const verify = async () => {
      const base = loadBaseline(params.baselineRun);
      const box = boxState(base.box.start);
      l3.box = { start: base.box.start, budgetMs: base.box.budgetMs ?? L3_BOX_MS, sourceRunId: base.runId, expiredAtStart: box.valid ? box.expired : null };
      if (box.valid && box.expired) finding(`the L3 box declared by baseline run ${base.runId} had expired when the verify phase started; L3c marks steps after the box NOT RUN`);
      pinsAndCli();
      const c = configNow();
      l3.config.hashes = c.hashes;
      l3.config.envSet = { CLAUDE_CONFIG_DIR: !!process.env.CLAUDE_CONFIG_DIR, CODEX_HOME: !!process.env.CODEX_HOME };
      l3.config.compare = compareSections(base.hashes, c.hashes);
      const notRestored = l3.config.compare.files.filter((f) => !['unchanged', 'absent'].includes(f.status)).map((f) => f.label);
      l3.steps.B7 = { status: 'recorded', againstBaseline: base.runId, notRestored, note: 'B7 teardown and restore are operator steps outside the driver; this is the hash comparison only' };
      for (const f of notRestored) finding(`B7: ${f} differs from its B0 baseline after the operator's teardown (not restored, or changed since)`);
    };

    // --- phase: probe (B1 record, B2, B3, B4) --------------------------------------------------------
    let serverDir = null;
    let clientDir = null;
    let claude = null;
    let codex = null;
    let threadId = null;
    const probe = async () => {
      const launch = ctx.launch;
      if (JSON.stringify(launch) !== JSON.stringify(G5_LAUNCH)) throw new DriverError(`launch ${JSON.stringify(launch)} is not the G5 launch ${JSON.stringify(G5_LAUNCH)} the staged channel server is registered for`);
      const base = loadBaseline(params.baselineRun);
      const box = boxState(base.box.start);
      l3.box = { start: base.box.start, budgetMs: base.box.budgetMs ?? L3_BOX_MS, sourceRunId: base.runId, remainingAtStartMs: box.valid ? box.remainingMs : null };
      if (!box.valid) throw new DriverError('the baseline record\'s box start is not a time');
      if (box.expired) stop(`the L3 box declared by baseline run ${base.runId} at ${base.box.start} has expired (60 minutes, L1 §12, never extended); the probe does not start. A new box needs a new baseline`);
      const boxEnd = Date.parse(box.end);
      remaining = () => Math.min(ctx.remainingMs(), boxEnd - Date.now());
      if (boxEnd - Date.now() < ctx.remainingMs()) l3.box.boundsRun = true;
      const bctx = { ...ctx, remainingMs: () => remaining() };
      const boxCheck = (what) => {
        if (Date.now() >= boxEnd) stop(`the L3 box expired during the probe (${what}); nothing further is sent`);
      };

      await requireBeaconVersion('B1');
      const { cli, xv } = pinsAndCli();
      if (!cli.claude || !cli.codex) stop(`a harness CLI could not be run (${JSON.stringify(l3.versions.cliOutput)}); nothing launched`);
      await beaconStatus('B1');

      // 1. B1 record: re-hash against B0. NOT RUN as a step (operator decision 2026-09-30).
      const c = configNow();
      l3.config.hashes = c.hashes;
      l3.config.envSet = { CLAUDE_CONFIG_DIR: !!process.env.CLAUDE_CONFIG_DIR, CODEX_HOME: !!process.env.CODEX_HOME };
      l3.config.compare = compareSections(base.hashes, c.hashes);
      l3.steps.B1 = {
        status: 'NOT RUN',
        reason: 'operator decision 2026-09-30 on #168: Beacon (Windows MSI, system-mode install) was installed before any B0 baseline; no uninstall and reinstall',
        evidence: 'L1 §12 2026-09-30 MSI findings; the `.beacon.bak` backups are weaker evidence (the MSI\'s second pass can overwrite them) and are not read by the driver',
        baselineVsProbeStart: l3.config.compare,
        attribution: 'any difference is attributed to the operator\'s Beacon step (or the harness itself), never to the driver, which writes no harness config',
      };
      if (!l3.config.compare.unchanged) finding(`harness config differs between the baseline and the probe start (${l3.config.compare.files.filter((f) => f.status !== 'unchanged' && f.status !== 'absent').map((f) => f.label).join(', ')}): attributed to the operator's Beacon step, never to the driver`);

      // 2. runtime.jsonl offset(s).
      const logDir = dirname(beaconLog);
      const logName = beaconLog.slice(logDir.length + 1);
      const siblings = () => {
        try {
          return readdirSync(logDir).filter((n) => n !== logName && n.startsWith(logName.replace(/\.jsonl$/, '')) && /\.jsonl/.test(n)).sort();
        } catch {
          return [];
        }
      };
      const probeStart = Date.now();
      const startOffset = existsSync(beaconLog) ? statSync(beaconLog).size : 0;
      l3.beacon.log = { ...l3.beacon.log, existsAtStart: existsSync(beaconLog), startOffset, siblingsAtStart: siblings() };
      if (!l3.beacon.log.existsAtStart) finding('Beacon\'s runtime log does not exist at the probe start; every snapshot will read zero hits (a result, not NOT RUN)');
      let prevEnd = startOffset;

      // 3. probe values: generated here, redacted everywhere.
      markers = makeProbeMarkers();
      for (const m of markers) {
        const P = m.id.toUpperCase().replace(/-/g, '_');
        ctx.redactLiteral(m.marker, `<L3_MARKER_${P}>`);
        ctx.redactLiteral(m.token, `<L3_FAKE_TOKEN_${P}>`);
      }
      l3.markers = markerRecords(markers);

      // 4. stage into scratch; augment ONLY the staged case table.
      serverDir = ctx.dir('l3-server');
      clientDir = ctx.dir('l3-client');
      const committedCases = readFileSync(join(REPO, GATE_SERVERS_DIR, 'g5-cases.json'));
      const staged = [...stageGateFiles(REPO, G5_SERVER_FILES, serverDir), ...stageGateFiles(REPO, G5_CLIENT_FILES, clientDir)];
      if (!staged.every((s) => s.match)) throw new DriverError('a staged gate-server copy does not match its source (sha256)');
      const table = JSON.parse(readFileSync(join(serverDir, 'g5-cases.json'), 'utf8'));
      const augmented = Buffer.from(`${JSON.stringify(augmentCaseTable(table, markers), null, 2)}\n`);
      writeFileSync(join(serverDir, 'g5-cases.json'), augmented);
      writeFileSync(join(clientDir, 'g5-cases.json'), augmented);
      l3.staging = {
        files: staged.map((s) => ({ path: s.path, headCommit: s.headCommit, committedSha256: s.committedSha256, workingTreeMatchesHead: s.workingTreeMatchesHead, stagedSha256: s.copySha256 })),
        casesStagedSha256: staged.find((s) => s.path.endsWith('g5-cases.json'))?.copySha256 ?? null,
        casesAugmentedSha256: sha256(augmented),
        committedCasesUnchanged: sha256(readFileSync(join(REPO, GATE_SERVERS_DIR, 'g5-cases.json'))) === sha256(committedCases),
        cases: { claude: L3_CLAUDE_CASE, codex: L3_CODEX_CASE, queueAdd: 'X4' },
      };
      const projectDir = ctx.dir('l3-project');
      const codexProjectDir = ctx.dir('l3-codex-project');
      const projectMcp = { mcpServers: { g5spike: { command: process.execPath, args: [join(serverDir, 'g5-channel.mjs')] } } };
      writeFileSync(join(projectDir, '.mcp.json'), `${JSON.stringify(projectMcp, null, 2)}\n`);

      // Operator texts: never a probe value, a spoofing body or a frame marker.
      const g5cases = JSON.parse(committedCases.toString('utf8'));
      const operator = { replyPrompt: params.replyPrompt, threadMarker: params.threadMarker };
      for (const [k, v] of Object.entries(operator)) {
        try {
          assertNoSpoof(k, v, g5cases);
          assertNoMarkerLeak(v, markers, k);
        } catch (err) {
          throw new DriverError(err.message);
        }
        if (/L3-PROBE-|sk-l3fake-/i.test(v)) throw new DriverError(`${k} carries a probe-value prefix; herdr never types one`);
      }
      l3.operatorTexts = operator;

      // Log snapshots: cumulative from the probe start, and the delta since the last snapshot.
      const snapshot = (step, extra = {}) => {
        const at = new Date().toISOString();
        if (!existsSync(beaconLog)) return { step, at, present: false, ...extra };
        const buf = readFileSync(beaconLog);
        const pick = (s) => ({ byMarker: s.byMarker, counts: hitCounts(s.hits, markers), bySession: s.bySession, warnings: s.warnings });
        const cumulative = pick(scanRuntimeLog([{ label: logName, text: buf, fromByte: startOffset }], markers));
        const delta = pick(scanRuntimeLog([{ label: logName, text: buf, fromByte: prevEnd }], markers));
        const snap = { step, at, present: true, bytes: buf.length, fromByte: startOffset, deltaFromByte: prevEnd, cumulative, delta, ...extra };
        prevEnd = buf.lastIndexOf(0x0a) + 1;
        return snap;
      };
      // Polls, bounded by beaconSettleMs and the box, for a NEW hit on pathId since the last
      // snapshot; with `actions`, for a new hit carrying one of those event.action values (B3
      // waits for the tool invocation itself, so a late B2 line cannot end its wait early).
      const pollLog = async (pathId, step, { actions = null } = {}) => {
        const t0 = Date.now();
        const deadline = t0 + Math.min(num('beaconSettleMs'), Math.max(0, remaining()));
        for (;;) {
          aborted();
          const buf = existsSync(beaconLog) ? readFileSync(beaconLog) : Buffer.alloc(0);
          const s = scanRuntimeLog([{ label: logName, text: buf, fromByte: prevEnd }], markers);
          const hit = actions ? s.hits.some((h) => h.markerId === pathId && actions.includes(h.eventAction)) : s.byMarker[pathId]?.lines > 0;
          if (hit) return snapshot(step, { hitSeenAfterMs: Date.now() - t0 });
          if (Date.now() >= deadline) return snapshot(step, { hitSeenAfterMs: null, waitedMs: Date.now() - t0 });
          await sleep(Math.min(num('pollMs'), Math.max(0, deadline - Date.now())));
        }
      };
      const syncHits = async (key, step) => {
        if (cliMode !== 'readonly') return { step, status: 'NOT RUN', reason: 'beaconCli=off: the poll path is not read' };
        const [file, args] = beaconCommand(key);
        aborted();
        const harness = key === 'claudeSync' ? 'claude' : 'codex';
        const r = await streamScan(file, args, { deadlineMs: Math.min(num('beaconSyncTimeoutMs'), Math.max(1, remaining())), env: process.env, abortSignal, needles: markers.flatMap((m) => [m.marker, m.token]) });
        l3.beacon.calls.push({ key, argv: ['beacon', ...args], step, startedAt: r.startedAt, endedAt: r.endedAt, exitCode: r.exitCode, timedOut: r.timedOut, spawnError: r.spawnError, stdoutBytes: r.bytes, stderrBytes: r.stderrBytes, streamed: true });
        aborted();
        const base = { step, harness, collection: 'poll (sync --print)', exitCode: r.exitCode, lines: r.lines, bytes: r.bytes, streamed: true };
        // Never stop() here: a failed poll read is NOT RUN for the poll path only (#195 review).
        if (r.timedOut || r.spawnError || r.exitCode !== 0) {
          const reason = r.timedOut ? `timed out after ${num('beaconSyncTimeoutMs')} ms (or the L3 box ran out)` : r.spawnError ? `could not start (${r.spawnError})` : `exited ${r.exitCode ?? r.signal}`;
          finding(`\`beacon ${BEACON_ALLOWLIST[key].join(' ')}\` at ${step}: ${reason}; the ${step} poll path is recorded NOT RUN and the probe goes on`);
          return { ...base, status: 'NOT RUN', reason };
        }
        const s = scanRuntimeLog(r.kept.join('\n'), markers);
        return { ...base, status: 'recorded', truncated: false, byMarker: s.byMarker, counts: hitCounts(s.hits, markers), bySession: s.bySession };
      };

      // herdr launch waits are bounded by the L3 box too (#195 review).
      const startupBound = () => Math.max(1000, Math.min(num('startupTimeoutMs'), Math.max(1, remaining())));
      const agentArgs = { ctx: bctx, g: l3, accept, num, stop };
      claude = makeAgent({ ...agentArgs, name: 'l3claude', label: 'claude', classify: (t) => classifyScreen(t, { busyIndicator: params.busyIndicator }), dialogKinds: DIALOG_KINDS, driverMayAccept: driverMayAcceptExpecting(() => Object.keys(projectMcp.mcpServers)) });
      codex = makeAgent({ ...agentArgs, name: 'l3codex', label: 'codex', classify: (t) => classifyCodexScreen(t, { busyIndicator: params.busyIndicator }), dialogKinds: CODEX_DIALOG_KINDS, driverMayAccept: driverMayAcceptCodex });
      const serverFacts = () => g5ClaudeFacts(existsSync(join(serverDir, 'transcript.jsonl')) ? parseJsonl(readFileSync(join(serverDir, 'transcript.jsonl'), 'utf8'), { completeLinesOnly: true }) : []);
      const clientEntries = () => (existsSync(join(clientDir, 'transcript.jsonl')) ? parseJsonl(readFileSync(join(clientDir, 'transcript.jsonl'), 'utf8'), { completeLinesOnly: true }) : []);
      const clientFacts = () => g5CodexFacts(clientEntries(), { question: '' });
      const lastLine = () => clientEntries().at(-1)?.line ?? 0;
      const sent = new Set();
      const sendOnce = (what) => {
        if (sent.has(what)) throw new DriverError(`refusing to send ${what} a second time; nothing is re-sent`);
        sent.add(what);
        (l3.injectionsSent ??= []).push({ what, at: new Date().toISOString() });
      };
      const codexCli = async (args, label) => {
        aborted();
        const [file, argv] = process.platform === 'win32' ? ['cmd.exe', ['/d', '/s', '/c', 'codex', ...args]] : ['codex', args];
        const r = await runBounded(file, argv, { deadlineMs: Math.min(num('cliTimeoutMs'), Math.max(1, remaining())), env: process.env, abortSignal });
        aborted();
        if (r.timedOut) stop(`\`codex ${args.join(' ')}\` timed out after ${num('cliTimeoutMs')} ms; nothing re-run`);
        return { r, rec: { label, argv: ['codex', ...args], startedAt: r.startedAt, endedAt: r.endedAt, exitCode: r.exitCode, timedOut: r.timedOut, spawnError: r.spawnError } };
      };
      const runClient = async (mode, args) => {
        aborted();
        const linesBefore = lastLine();
        const r = await runBounded(process.execPath, [join(clientDir, 'g5-codex.mjs'), mode, ...args], { deadlineMs: Math.min(num('clientTimeoutMs'), Math.max(1, remaining())), env: process.env, cwd: clientDir, abortSignal });
        const rec = { mode, args, startedAt: r.startedAt, endedAt: r.endedAt, exitCode: r.exitCode, timedOut: r.timedOut, stdoutStatus: String(r.stdout ?? '').split('\n').filter((l) => /^\[(?:done|error|upgrade failed)\]/.test(l)).map((l) => cap(l, 200)), linesBefore, linesAfter: lastLine() };
        (l3.clientRuns ??= []).push(rec);
        aborted();
        if (r.timedOut) stop(`the client's \`${mode}\` run timed out; nothing re-sent`);
        if (r.spawnError) throw new DriverError(`could not run the staged client (${r.spawnError})`);
        const conns = g5CodexFacts(clientEntries().filter((e) => e.line > linesBefore), { question: '' }).wire.connections;
        const problems = [];
        if (!conns.length) problems.push('no connection on the wire');
        for (const cn of conns) {
          if (cn.upgraded === false) problems.push(`WebSocket upgrade refused: ${cn.handshake}`);
          for (const e of cn.errors) problems.push(`${e.method} answered with error ${JSON.stringify(e.error)}`);
        }
        if (r.exitCode !== 0) problems.push(`client exited ${r.exitCode ?? r.signal}`);
        rec.problems = problems;
        rec.userAgentVersion = conns[0]?.userAgentVersion ?? null;
        if (problems.length) {
          finding(`the staged G5 client did not work unmodified on this Codex version: \`${mode}\`: ${problems.join('; ')}. Not patched`);
          throw new DriverError(`the staged G5 client's \`${mode}\` failed: ${problems.join('; ')}`);
        }
        return rec;
      };

      // 5. B2: Claude channel delivery.
      boxCheck('before B2');
      boxCheck('before the Claude launch');
      const ws = await herdr.workspaceCreate({ cwd: projectDir, label: 'oac-l3-claude' });
      ctx.record('workspace', ws);
      await herdr.paneProcessInfo(ws.paneId);
      await ctx.probeEnv(ws.paneId);
      const started = await ctx.startAgent('l3claude', { paneId: ws.paneId, timeoutMs: startupBound(), allowErrorCodes: ['agent_not_ready'] });
      l3.claudeStart = { seq: herdr.commands.at(-1).seq, errorCode: started.errorCode, herdrReportedArgv: started.argv };
      await claude.settle('startup', num('startupTimeoutMs'));
      const hs = await claude.waitFor('the channel-server handshake', () => {
        const f = serverFacts();
        const init = f.initialize.find((i) => !i.error);
        return init && f.toolsList.length && f.armed.length ? { f, init } : null;
      }, num('handshakeTimeoutMs'), { lbl: 'handshake-wait' });
      l3.handshake = { initialize: { requested: hs.init.requested, negotiated: hs.init.negotiated, channelCap: hs.init.channelCap, permissionCap: hs.init.permissionCap, clientVersion: hs.init.clientVersion } };
      l3.versions.claudeWire = hs.init.clientVersion;
      if (hs.init.clientVersion !== cli.claude) finding(`Claude Code's wire clientInfo.version (${hs.init.clientVersion}) differs from \`claude --version\` (${cli.claude})`);
      await claude.settle('post-handshake', num('startupTimeoutMs'));
      boxCheck('B2 trigger');
      const pre = await claude.read('pre-L3C-idle-check');
      if (pre.screen.busy || pre.screen.dialog) stop('the Claude session was not visibly idle before the probe case');
      // #253: a baseline and an activity watch before the push (the settle waits for its turn).
      const pushTurn = await claude.watch('L3C-push');
      sendOnce(`Claude case ${L3_CLAUDE_CASE} (case.trigger)`);
      writeFileSync(join(serverDir, 'case.trigger'), `${L3_CLAUDE_CASE}\n`);
      const w = await claude.waitFor(`case ${L3_CLAUDE_CASE} on the wire`, () => {
        const f = serverFacts();
        const n = f.notifications.find((x) => x.case === L3_CLAUDE_CASE);
        const r = f.refused.find((x) => x.case === L3_CLAUDE_CASE);
        return n ? { notification: n } : r ? { refused: r } : null;
      }, num('wireTimeoutMs'), { lbl: 'L3C-wait' });
      if (w.refused) stop(`the channel server refused case ${L3_CLAUDE_CASE} before sending it (pre-send check)`);
      // #246/#253: every full read after a turn is a settled read (gate-common settledRead).
      const afterB2 = await claude.settledRead('after-L3C', { source: 'recent-unwrapped', lines: num('readLines') }, { context: 'L3C-turn', timeoutMs: num('turnTimeoutMs'), since: pushTurn });
      const b2Log = await pollLog('claude-channel', 'B2');
      l3.steps.B2 = { status: 'recorded', wire: { line: w.notification.line, t: w.notification.t }, afterReadSeq: afterB2.seq, dialogs: l3.dialogs.filter((d) => d.agent === 'claude').map((d) => ({ kind: d.kind, acceptOrigin: d.acceptOrigin, acceptKeys: (d.acceptKeys ?? []).map((k) => k.key) })), log: b2Log };
      l3.beacon.sync.B2 = await syncHits('claudeSync', 'B2');

      // 6. B3: Claude answers through the reply tool. The typed text carries no probe value.
      boxCheck('B3 prompt');
      const repliesBefore = serverFacts().replyCalls.length;
      const q = await claude.prompt(operator.replyPrompt, { wait: true });
      // #253: typed with `herdr agent prompt --wait` (herdr observed the prompt's own turn).
      const afterB3 = await claude.settledRead('after-B3', { source: 'recent-unwrapped', lines: num('readLines') }, { context: 'B3-turn', timeoutMs: num('turnTimeoutMs'), since: q });
      const replies = serverFacts().replyCalls.slice(repliesBefore);
      const cm = markers.find((m) => m.id === 'claude-channel');
      const b3Log = await pollLog('claude-channel', 'B3', { actions: TOOL_INVOKED_ACTIONS });
      l3.steps.B3 = {
        status: 'recorded',
        promptSeq: q.seq,
        afterReadSeq: afterB3.seq,
        replyToolCalls: replies.length,
        replyArgsCarryMarker: replies.some((x) => JSON.stringify(x.args ?? {}).includes(cm.marker)),
        replyArgsCarryToken: replies.some((x) => JSON.stringify(x.args ?? {}).includes(cm.token)),
        log: b3Log,
      };
      if (!replies.length) finding('B3: Claude did not call the channel server\'s reply tool in answer to the operator prompt; recorded as a result');
      l3.beacon.sync.B3 = await syncHits('claudeSync', 'B3');

      // 7. B4: Codex app-server input (turn/start and thread/queue/add) against the shared daemon.
      boxCheck('B4 daemon');
      const dvBefore = await codexCli(['app-server', 'daemon', 'version'], 'daemon version (before start)');
      const runningBefore = dvBefore.r.exitCode === 0 && !!parseCodexDaemonVersion(dvBefore.r.stdout);
      ctx.noteScratchHolder?.(CODEX_DAEMON_SCRATCH_HOLDER);
      const ds = await codexCli(['app-server', 'daemon', 'start'], 'daemon start');
      let startStatus = null;
      try {
        startStatus = JSON.parse(String(ds.r.stdout).trim().split('\n').at(-1)).status ?? null;
      } catch {
        /* not JSON */
      }
      l3.daemon = { versionBefore: { ...dvBefore.rec, running: runningBefore }, start: { ...ds.rec, status: startStatus } };
      if (ds.r.spawnError || ds.r.exitCode !== 0) throw new DriverError(`\`codex app-server daemon start\` failed (${ds.r.spawnError ?? `exit ${ds.r.exitCode}`})`);
      const alreadyRunning = runningBefore || startStatus === 'already running';
      l3.daemon.alreadyRunning = alreadyRunning;
      // The driver never stops the daemon: it is left running with Beacon's [otel] config
      // loaded, and the operator stops it (`codex app-server daemon stop`) before B7.
      l3.daemon.leftRunning = true;
      l3.daemon.operatorNote = 'the Codex app-server daemon is left running after the probe with the Beacon [otel] config loaded; run `codex app-server daemon stop` before the B7 teardown';
      finding(l3.daemon.operatorNote);
      finding(alreadyRunning
        ? 'Codex app-server daemon state: ALREADY RUNNING before the probe. A daemon started before Beacon\'s [otel] write to config.toml may not export OTLP to Beacon, so B4\'s OTLP-path counts may read zero for that reason (the operator was to stop it before the probe)'
        : 'Codex app-server daemon state: not running before the probe; the driver started it (`codex app-server daemon start`), after Beacon\'s config was in place');
      const dv = await codexCli(['app-server', 'daemon', 'version'], 'daemon version');
      const daemonV = parseCodexDaemonVersion(dv.r.stdout);
      l3.daemon.version = { ...dv.rec, parsed: daemonV };
      l3.versions.codex.daemon = daemonV;
      for (const k of CODEX_DAEMON_VERSION_FIELDS) {
        const w = codexVersionWarning({ observed: daemonV?.[k] ?? null, lastTested: xv.lastTested, minimum: xv.minimum, source: `\`codex app-server daemon version\` ${k}`, gate: 'L3' });
        if (w) finding(w);
      }
      const preList = await runClient('list', []);
      // #205 review: the ready wait's baseline, from the pre-launch list's own lines; without it
      // nothing is launched (a [] fallback would count an already-loaded thread as new).
      const preLoaded = loadedSince(clientFacts().wire.loadedLists, preList.linesBefore);
      l3.codexPreLaunchLoaded = preLoaded === null ? null : preLoaded.length;
      if (preLoaded === null) stop('the pre-launch `thread/loaded/list` could not be read, so the ready wait (#204) could not tell a thread new since the launch; the Codex TUI was not launched');
      l3.versions.codex.wire = preList.userAgentVersion;
      const wireWarning = codexVersionWarning({ observed: preList.userAgentVersion ?? null, lastTested: xv.lastTested, minimum: xv.minimum, source: 'the Codex wire initialize userAgent', gate: 'L3' });
      if (wireWarning) finding(wireWarning);

      boxCheck('before the Codex launch');
      const cws = await herdr.workspaceCreate({ cwd: codexProjectDir, label: 'oac-l3-codex' });
      ctx.record('codexWorkspace', cws);
      await herdr.paneProcessInfo(cws.paneId);
      const cstart = await herdr.agentStart('l3codex', { launchArgv: [...G2_LAUNCH], paneId: cws.paneId, timeoutMs: startupBound(), allowErrorCodes: ['agent_not_ready'] });
      l3.codexStart = { seq: herdr.commands.at(-1).seq, errorCode: cstart.errorCode, herdrReportedArgv: cstart.argv, launch: [...G2_LAUNCH] };
      // One process table for the pane's whole tree, the one the driver took for its query
      // (#136 review, #244 note D: each table is one WMI query, ~1.6 s, on Windows).
      const { info, table: procTable } = await herdr.paneProcessSnapshot(cws.paneId);
      const fg = (info.foreground_processes ?? []).map((p) => p.pid).filter(Number.isInteger);
      const tree = [...new Set([...fg, ...fg.flatMap((p) => descendants(p, procTable) ?? []), ...(descendants(info.shell_pid, procTable) ?? [])])];
      const { proof } = paneArgv(tree, procTable); // #232: argsAfterCodex minimized
      l3.codexPaneArgv = { proof };
      if (proof.found && !proof.plain) throw new DriverError(`the Codex pane's process runs with arguments ${JSON.stringify(proof.argsAfterCodex)}; L3's Codex side is plain \`codex\` attached to the shared daemon`);
      if (!proof.found) finding('the Codex pane\'s process argv could not show a `codex` process; the plain launch rests on herdr\'s reported argv only');
      await codex.settle('codex-startup', num('startupTimeoutMs'));
      // #204: the composer Codex 0.159.2 shows at startup is its startup draft, not a session.
      // The thread marker is typed only once a new thread is loaded in the daemon and the pane
      // shows the idle composer (lib/g2.mjs codexReadiness); a startup screen the driver may not
      // answer (the hook review) ends the run NOT RUN naming it; nothing is typed or re-sent.
      // #205 review: under accept=human the operator answered Codex's hook review; they may have
      // chosen "Continue without trusting", in which case Beacon's SessionStart hook did not run
      // in this session and B4's hook-path counts can be missing for that reason.
      const hookReviewFinding = () => {
        const hd = l3.dialogs.filter((d) => d.agent === 'codex' && ['hooks-review', 'hooks-browser'].includes(d.kind) && d.acceptOrigin === 'human');
        if (hd.length) finding(`Codex's startup hook review was on screen and answered by the operator (${hd.map((d) => `${d.kind}, read #${d.readSeq}`).join('; ')}); the driver cannot see which option was chosen. If it was "Continue without trusting", Beacon's SessionStart hook did not run in this Codex session, so B4's hook-path (\`collection_method: hook\`) counts may be missing for that reason`);
      };
      let ready;
      try {
        ready = await waitCodexReady({
          read: codex.read,
          handleDialog: codex.handleDialog,
          listLoaded: async () => {
            const { linesBefore } = await runClient('list', []);
            return loadedSince(clientFacts().wire.loadedLists, linesBefore); // read after the poll: its own answer only
          },
          preLoaded,
          timeoutMs: num('readyTimeoutMs'),
          pollMs: num('listPollMs'),
          remainingMs: remaining,
          stop,
          onTimeout: (v) => {
            const f = codexReadyTimeoutFinding(v);
            if (f) finding(f);
          },
        });
      } finally {
        hookReviewFinding();
      }
      l3.codexReady = { readSeq: ready.readSeq, newThreads: ready.newThreads.length, polls: ready.polls, waitedMs: ready.waitedMs, observations: ready.observations };
      if (ready.newThreads.length > 1) finding(multipleNewThreadsFinding(ready.newThreads.length));
      // #282: the session loaded on the wire is not the end of Codex's startup; settle first.
      l3.codexStartupSettle = await codex.startupSettle('codex-ready-settle', `the Codex session loaded in the daemon (pane read #${ready.readSeq})`, num('turnTimeoutMs'));
      const tm = await codex.prompt(operator.threadMarker, { wait: true });
      // #253: typed with `herdr agent prompt --wait`, so herdr observed the marker's own turn;
      // never a wait that returns at once with the state from before the prompt.
      const mr = await codex.settle('thread-marker-turn', num('turnTimeoutMs'), { since: tm });
      const projectDirs = [...new Set([codexProjectDir, realpathSync(codexProjectDir)])];
      const attachDeadline = Date.now() + Math.min(num('attachTimeoutMs'), Math.max(0, remaining()));
      let found;
      for (;;) {
        await runClient('list', []);
        found = identifyTuiThread(clientFacts().wire, { operatorPrompt: operator.threadMarker, projectDirs, sinceLine: 0 });
        if (found.threadId) break;
        if (found.candidates.length > 1) throw new DriverError(`${found.why}: ${found.candidates.map((x) => x.id).join(', ')}`);
        if (Date.now() + num('listPollMs') >= attachDeadline) {
          finding('no thread loaded in the daemon matched the thread marker and the Codex project directory: the TUI may not have attached to the daemon');
          stop(`no loaded thread matched the operator's Codex thread within ${num('attachTimeoutMs')} ms; nothing delivered`);
        }
        await sleep(num('listPollMs'));
      }
      threadId = found.threadId;
      l3.thread = { id: threadId, markerPromptSeq: tm.seq, markerSettledSeq: mr.seq, markerSettledBy: mr.settled?.turnBegunBy ?? null };
      // #253: the marker's turn must be over on the wire (thread/turns/list: no turn in
      // progress, the marker's turn completed) before B4's turn/start, or it joins that turn.
      {
        let next = 0;
        l3.thread.markerIdle = await codex.waitFor('the thread-marker turn to complete (thread/turns/list)', async () => {
          if (Date.now() < next) return null;
          next = Date.now() + num('listPollMs');
          const { linesBefore } = await runClient('turns', [threadId]);
          return threadIdleOnWire(clientFacts(), threadId, { marker: operator.threadMarker, sinceLine: linesBefore });
        }, num('turnTimeoutMs'), { lbl: 'codex-idle-wait' });
      }
      const turnDone = (caseId, what) => {
        let next = 0;
        return codex.waitFor(what, async () => {
          if (Date.now() < next) return null;
          next = Date.now() + num('listPollMs');
          await runClient('turns', [threadId]);
          const cs = clientFacts().cases.find((x) => x.case === caseId);
          return cs?.turnId && cs.turnStatus === 'completed' ? cs : null;
        }, num('turnTimeoutMs'), { lbl: 'codex-turn-wait' });
      };
      boxCheck('B4 turn/start');
      sendOnce(`Codex case ${L3_CODEX_CASE} (turn/start, client)`);
      await runClient('deliver', [threadId, L3_CODEX_CASE]);
      const t1 = await turnDone(L3_CODEX_CASE, `the Codex turn for ${L3_CODEX_CASE} to complete (thread/turns/list)`);
      boxCheck('B4 thread/queue/add');
      sendOnce('Codex case X4 (setup turn/start + thread/queue/add, client)');
      await runClient('x4', [threadId]);
      const t2 = await turnDone('X4', 'the queued X4 input\'s turn to complete (thread/turns/list)');
      const afterB4 = await codex.settledRead('after-B4', { source: 'recent-unwrapped', lines: num('readLines') }, { context: 'B4-turns', timeoutMs: num('turnTimeoutMs'), done: async () => `the X4 queued turn ${t2.turnId} completed on the wire (thread/turns/list)` });
      const b4Log = await pollLog('codex-queue-add', 'B4');
      l3.steps.B4 = { status: 'recorded', turnStart: { turnId: t1.turnId, status: t1.turnStatus, recordedByteIdentical: t1.recordedByteIdentical }, queueAdd: { turnId: t2.turnId, status: t2.turnStatus, recordedByteIdentical: t2.recordedByteIdentical }, afterReadSeq: afterB4.seq, log: b4Log };
      l3.beacon.sync.B4 = await syncHits('codexSync', 'B4');

      // 8. The whole-run scan: runtime.jsonl from its offset, and any rotated sibling touched
      // since the probe started (a probe value cannot predate the probe, so a sibling is read
      // whole).
      const inputs = [];
      if (existsSync(beaconLog)) inputs.push({ label: logName, text: readFileSync(beaconLog), fromByte: startOffset });
      for (const n of siblings()) {
        try {
          if (statSync(join(logDir, n)).mtimeMs >= probeStart - 1000) inputs.push({ label: n, text: readFileSync(join(logDir, n)), fromByte: 0 });
        } catch {
          /* vanished */
        }
      }
      const scan = scanRuntimeLog(inputs, markers);
      const redactor = createRedactor();
      const excerpts = scan.hits.slice(0, 100).map((h) => {
        const buf = inputs.find((i) => i.label === h.file).text;
        const end = buf.indexOf(0x0a, h.byteOffset);
        const line = buf.subarray(h.byteOffset, end === -1 ? buf.length : end).toString('utf8');
        return { file: h.file, line: h.line, markerId: h.markerId, eventAction: h.eventAction, text: manifestSafePlaceholders(redactedExcerpt(line, h, markers, redactor)) };
      });
      l3.scan = { files: scan.files, byMarker: scan.byMarker, counts: hitCounts(scan.hits, markers), bySession: scan.bySession, warnings: scan.warnings, hitCount: scan.hits.length, excerpts, excerptsTruncated: scan.hits.length > excerpts.length, excerptsNote: 'one redactedExcerpt() per hit (first 100); placeholders are written {L3-MARKER <id>} / {L3-FAKE-TOKEN <id>} (manifest-safe form of lib/l3.mjs PLACEHOLDER_RE)' };
      for (const wng of scan.warnings) finding(`runtime log scan: ${wng}`);
      const probeSessions = new Set(scan.bySession.filter((s) => s.markerIds.length).map((s) => `${s.harness} ${s.sessionId}`));
      l3.scan.sessionsHoldingProbeValues = probeSessions.size;

      // Claude session file: entry types and flags only (operator decision 2026-09-30).
      if (params.readSessionFile === 'true') {
        const claudeHome = process.env.CLAUDE_CONFIG_DIR || join(homedir(), '.claude');
        const dirs = [...new Set([projectDir, realpathSync(projectDir)].map((d) => join(claudeHome, 'projects', claudeProjectSlug(d))))];
        const sf = { read: true, source: process.env.CLAUDE_CONFIG_DIR ? '$CLAUDE_CONFIG_DIR/projects/<slug>' : '~/.claude/projects/<slug>', dirsFound: 0, files: 0, lines: 0, entries: [] };
        for (const d of dirs) {
          if (!existsSync(d)) continue;
          sf.dirsFound += 1;
          for (const n of readdirSync(d).filter((x) => x.endsWith('.jsonl'))) {
            const shapes = sessionEntryShapes(readFileSync(join(d, n), 'utf8'), markers);
            sf.files += 1;
            (sf.sessionIds ??= []).push(n.replace(/\.jsonl$/, ''));
            sf.lines += shapes.lines;
            sf.entries.push(...shapes.entries);
          }
        }
        if (!sf.dirsFound) finding('B2 session file: no Claude project directory was found for the scratch probe project (slug rule UNVERIFIED); session-file shape not recorded');
        l3.sessionFile = sf;
      } else {
        l3.sessionFile = { read: false, note: 'not read (--param readSessionFile is not true); the poll path (`sync --print`) is the B2 session-file evidence' };
      }

      // The probe's own sessions per harness (L3c contract): the harness-side ids the driver
      // knows, independent of Beacon: the Claude session file name(s) in the scratch probe
      // project (null when readSessionFile is off) and the Codex TUI thread id. That Beacon's
      // session.id equals these ids is UNVERIFIED; a hit whose session.id is none of them is
      // "outside the probe sessions" only under that assumption.
      l3.probeSessions = probeSessionIds(l3.sessionFile, threadId);

      l3.steps.B5 = { status: 'NOT RUN', reason: 'B5 runs Beacon\'s own memory evaluation: a Beacon step only the operator may do (default taken on #168, 2026-09-30)' };
      l3.steps.B6 = { status: 'NOT RUN', reason: 'B6 needs Beacon\'s MCP registration and an approved memory: Beacon steps only the operator may do (default taken on #168, 2026-09-30)' };

      // End of the probe: Beacon must not have changed; harness changes are findings.
      const endV = await beaconVersionNow('end');
      l3.versions.beaconAtEnd = endV.version;
      if (endV.version !== l3.versions.beacon) stop(`Beacon's version changed during the probe (${l3.versions.beacon} -> ${endV.version ?? 'unreadable'}); the run is NOT RUN`);
      const post = await harnessVersions(['claude', 'codex'], { deadlineMs: Math.min(15000, Math.max(1, remaining())) });
      l3.versions.postRun = { claude: parseClaudeCliVersion(post.claude), codex: parseCodexCliVersion(post.codex) };
      if (l3.versions.postRun.claude !== cli.claude || l3.versions.postRun.codex !== cli.codex) finding(`a harness CLI version changed during the probe (${JSON.stringify(l3.versions.postRun)} after, ${JSON.stringify(cli)} before)`);
      boxCheck('end of the probe');
    };

    try {
      if (phase === 'baseline') await baseline();
      else if (phase === 'verify') await verify();
      else await probe();
    } finally {
      // Backstop: the record carries ids and hashes only.
      if (markers.length && findMarkerLeaks(JSON.stringify(l3), markers).length) {
        ctx.record('l3', { version: L3_RECORD_VERSION, phase, withheld: 'the L3 record held a probe value and was withheld' });
        // eslint-disable-next-line no-unsafe-finally
        throw new DriverError('the L3 record held a probe value; withheld');
      }
      ctx.record('l3', l3);
      if (phase === 'probe') {
        if (claude?.sections.length) ctx.capture('l3-pane-claude.txt', claude.sections.join(''));
        if (codex?.sections.length) ctx.capture('l3-pane-codex.txt', codex.sections.join(''));
        if (serverDir && existsSync(join(serverDir, 'transcript.jsonl'))) ctx.capture('l3-transcript-claude.jsonl', readFileSync(join(serverDir, 'transcript.jsonl'), 'utf8'), { format: 'jsonl' });
        if (clientDir && existsSync(join(clientDir, 'transcript.jsonl'))) {
          const s = sanitizeTranscript(readFileSync(join(clientDir, 'transcript.jsonl'), 'utf8'), { ownThreadId: threadId });
          for (const id of s.installationIds) ctx.redactLiteral(id, '<INSTALLATION_ID>');
          l3.sanitizer = s.report;
          ctx.capture('l3-transcript-codex.jsonl', s.text, { format: 'jsonl' });
        }
      }
    }
  },
};
