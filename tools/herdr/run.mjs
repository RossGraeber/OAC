#!/usr/bin/env node
// herdr driver (Epic K #123, K3 #126): runs one scenario against real harness CLIs through
// herdr and records the run well enough to sit beside a human-operated gate transcript.
//
//   node tools/herdr/run.mjs --scenario <name|path.mjs> [options]
//   node tools/herdr/run.mjs --self-test
//
// Options:
//   --scenario <name|path>   tools/herdr/scenarios/<name>.mjs, or a path to a scenario module
//   --launch '<json argv>'   launch command for the scenario (overrides its default)
//   --param key=value        scenario parameter (repeatable)
//   --timebox-ms <n>         run timebox (default: the scenario's, else 300000)
//   --out <dir>              where run-manifest.json and redacted captures go
//                            (default: <os tmpdir>/oac-herdr-runs/<run id>)
//   --keep-scratch           keep the raw scratch directory (unredacted captures, server log)
//   --redact-literal V=<P>   also redact run-specific value V as placeholder <P> (repeatable;
//                            e.g. an installation id); only <P> is recorded
//   --herdr-bin <path>       herdr executable (default: `herdr` on PATH; a .mjs path runs
//                            under node -- used by the self-test's fake herdr, and recorded
//                            as herdr.executable.testDouble: true)
//
// Outcome and exit code: PASS 0, FAIL 1, usage error 2, NOT RUN 3. A timeout, an expired
// timebox, an operator abort, or a herdr version other than the PINS.md pin is NOT RUN --
// never a failure and never a fabricated pass. The pin is read from PINS.md as committed at
// HEAD; an uncommitted edit to its herdr row is NOT RUN too, never applied (#139).
//
// The manifest records which executables ran (#140): herdr.executable and
// harnessExecutables (basename, sha256, format; never a directory), and each written
// capture's sha256.
//
// Test tooling only. Node built-ins only; no package.json. herdr is an external process,
// never linked. The driver never reads harness credentials, never writes harness config
// (it hashes it before and after), and never runs the herdr subcommand that adds hooks
// to a harness's config.
// Scope and boundaries: docs/planning/decisions/K1-herdr-evaluation.md,
// scripts/check-herdr-containment.mjs (oac-boundaries checks 9 and 10).

import { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, isAbsolute, join, relative, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { randomBytes } from 'node:crypto';

import { readCommittedHerdrPin, versionMatches, PIN_ROW } from './lib/pins.mjs';
import { HerdrSession, NotRunError, DriverError, makeSessionName } from './lib/herdr.mjs';
import {
  MANIFEST_SCHEMA_VERSION, HERDR_RUN_CONFIG, driverInfo, osInfo, hashHarnessConfig, compareHashes,
  probeHarnesses, herdrLaunchEnv, paneEnvDelta, HOST_HARNESS_ENV, resolveHerdr, herdrIdentity, sha256Text,
} from './lib/manifest.mjs';
import { createRedactor, reportIsClean, summarize, parseLiteralSpec } from './lib/redact.mjs';
import { defaultPaneShell, quoteCommand } from './lib/pane-shell.mjs';
import { killTree, within } from './lib/proc.mjs';
import { removeScratch } from './lib/scratch.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
export const REPO_ROOT = resolve(HERE, '..', '..');
const PROBE_PATH = join(HERE, 'lib', 'env-probe.mjs');

export const EXIT = Object.freeze({ PASS: 0, FAIL: 1, USAGE: 2, 'NOT RUN': 3 });

class UsageError extends Error {}

export function parseArgs(argv) {
  const o = { params: {} };
  const need = (i, flag) => {
    if (argv[i + 1] === undefined) throw new UsageError(`${flag} needs a value`);
    return argv[i + 1];
  };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--scenario') o.scenario = need(i++, a);
    else if (a === '--launch') {
      let v;
      try {
        v = JSON.parse(need(i++, a));
      } catch {
        throw new UsageError('--launch takes a JSON array of strings, e.g. \'["claude","--flag"]\'');
      }
      if (!Array.isArray(v) || !v.every((x) => typeof x === 'string')) throw new UsageError('--launch must be a JSON array of strings');
      o.launch = v;
    } else if (a === '--param') {
      const kv = need(i++, a);
      const eq = kv.indexOf('=');
      if (eq < 1) throw new UsageError('--param takes key=value');
      o.params[kv.slice(0, eq)] = kv.slice(eq + 1);
    } else if (a === '--timebox-ms') {
      o.timeboxMs = Number(need(i++, a));
      if (!(o.timeboxMs > 0)) throw new UsageError('--timebox-ms must be a positive number');
    } else if (a === '--out') o.out = need(i++, a);
    else if (a === '--keep-scratch') o.keepScratch = true;
    else if (a === '--herdr-bin') o.herdrBin = need(i++, a);
    else if (a === '--redact-literal') {
      try {
        (o.redactLiterals ??= []).push(parseLiteralSpec(need(i++, a)));
      } catch (err) {
        throw new UsageError(`--redact-literal: ${err.message}`);
      }
    }
    else if (a === '--self-test') o.selfTest = true;
    else if (a === '--help' || a === '-h') o.help = true;
    else throw new UsageError(`unknown argument ${a}`);
  }
  if (!o.help && !o.selfTest && !o.scenario) throw new UsageError('--scenario is required');
  return o;
}

export async function loadScenario(nameOrPath) {
  const path = /[\\/]|\.mjs$/.test(nameOrPath) ? resolve(nameOrPath) : join(HERE, 'scenarios', `${nameOrPath}.mjs`);
  if (!/^[A-Za-z0-9._\\/:-]+$/.test(nameOrPath) || !existsSync(path)) throw new UsageError(`no scenario "${nameOrPath}" (looked for ${path})`);
  const sc = (await import(pathToFileURL(path).href)).default;
  if (!sc || typeof sc.name !== 'string' || typeof sc.run !== 'function') throw new UsageError(`${path} does not export a default { name, run } scenario`);
  const rel = relative(REPO_ROOT, path);
  return { ...sc, file: rel.startsWith('..') || isAbsolute(rel) ? `(outside repo) ${path}` : rel.split('\\').join('/') };
}

export function herdrCommand(bin = 'herdr') {
  return /\.m?js$/i.test(bin) ? [process.execPath, resolve(bin)] : [bin];
}

export function isInside(dir, root) {
  const rel = relative(realpathSync(root), realpathSync(dir));
  return rel === '' || (!rel.startsWith('..') && !isAbsolute(rel));
}

const iso = (ms) => new Date(ms).toISOString();

// The driver phase an escaping error came from, for the console (#202): 'setup' (before the
// scenario body starts), 'run', 'teardown' (session teardown, scratch removal, hashes),
// 'record' (redaction and writing the manifest).
export const PHASES = Object.freeze(['setup', 'run', 'teardown', 'record']);

export async function runScenario(opts) {
  const state = { phase: 'setup' };
  try {
    return await runScenarioInner(opts, state);
  } catch (err) {
    if (err && typeof err === 'object' && !err.phase) err.phase = state.phase;
    throw err;
  }
}

async function runScenarioInner(opts, state) {
  const scenario = await loadScenario(opts.scenario);
  const launch = opts.launch ?? scenario.defaults?.launch ?? [];
  const params = { ...(scenario.defaults?.params ?? {}), ...opts.params };
  const timeboxMs = opts.timeboxMs ?? scenario.defaults?.timeboxMs ?? 300000;
  const runId = `${new Date().toISOString().replace(/[-:]/g, '').replace(/\.\d+Z$/, 'Z')}-${randomBytes(3).toString('hex')}`;
  const graceMs = 5000;

  // Scratch lives outside the repository, always; raw captures never land in the tree.
  // Everything between creating it and the main try/finally is guarded, so a setup error
  // cannot leak the directory.
  const scratch = mkdtempSync(join(tmpdir(), 'oac-herdr-scratch-'));
  let outDir;
  let herdr;
  let herdrEnv;
  let herdrResolved;
  let manifest;
  let ctx;
  let leftover = null;
  const commands = [];
  const captures = [];
  const extraLiterals = [...(opts.redactLiterals ?? [])];
  const abort = new AbortController();
  const timeboxStart = Date.now();
  const timebox = { remainingMs: () => timeboxStart + timeboxMs - Date.now() };
  const sessionName = makeSessionName(scenario.name);
  try {
    if (isInside(scratch, REPO_ROOT)) throw new DriverError('scratch directory resolved inside the repository; refusing to run');
    mkdirSync(join(scratch, 'captures'));
    outDir = resolve(opts.out ?? join(tmpdir(), 'oac-herdr-runs', runId));
    mkdirSync(outDir, { recursive: true });

    const configPath = join(scratch, 'herdr-config.toml');
    writeFileSync(configPath, HERDR_RUN_CONFIG);
    const launchEnv = herdrLaunchEnv(process.env, configPath);
    herdrEnv = launchEnv.env;
    // The herdr executable is resolved once, here, and spawned only by that absolute path
    // (#140): the manifest hashes the file resolved at run start, and re-hashes it at teardown.
    // A bare name is never spawned (a spawn's own PATH search could find a file the resolver
    // skipped, e.g. via a relative PATH entry or, on win32, the cwd): an unresolved herdr is
    // NOT RUN before anything is spawned (body()).
    const herdrCmd = herdrCommand(opts.herdrBin);
    herdrResolved = resolveHerdr(herdrCmd, { env: herdrEnv });
    herdr = new HerdrSession({
      herdrCmd: herdrResolved.runUnderNode ? herdrCmd : [herdrResolved.path ?? '<unresolved herdr; never spawned>'],
      sessionName,
      env: herdrEnv,
      cwd: scratch,
      timebox,
      commands,
      abortSignal: abort.signal,
      graceMs,
    });

    manifest = {
      schemaVersion: MANIFEST_SCHEMA_VERSION,
      runId,
      outcome: null,
      outcomeReason: null,
      scenario: { name: scenario.name, file: scenario.file, description: scenario.description ?? null, params },
      launch: { argv: launch, herdrReportedArgv: null },
      driver: driverInfo(REPO_ROOT),
      herdr: {
        pinRow: PIN_ROW,
        executable: null,
        pinnedTag: null,
        pinsSource: null,
        expectedVersionOutput: null,
        observedVersionOutput: null,
        serverStatus: null,
        config: { path: configPath, contents: HERDR_RUN_CONFIG },
        agentManifests: null,
      },
      harnessVersions: {},
      harnessExecutables: {},
      os: osInfo(),
      session: { name: sessionName, serverPid: null, panePids: [] },
      env: { serverLaunch: launchEnv.delta, pane: null },
      harnessConfig: { before: hashHarnessConfig(), after: null, unchanged: null, changed: [] },
      timebox: { budgetMs: timeboxMs, start: iso(timeboxStart), end: null, elapsedMs: null, expired: null, teardownEnd: null },
      commands,
      teardown: null,
      scratch: { location: 'os.tmpdir(), outside the repository', path: scratch, removed: null, holders: [] },
      captures: [],
      redaction: { runLiteralPlaceholders: [] },
      scenarioData: {},
      findings: [],
    };
    const hostVars = launchEnv.delta.removed.filter((k) => HOST_HARNESS_ENV.test(k));
    if (hostVars.length) {
      manifest.findings.push(
        `the driver was started from inside a Claude Code session: ${hostVars.length} of its variables (names in env.serverLaunch.removed) were removed ` +
          'so the harness under test does not run as that session\'s child or through its relay (#163)',
      );
    }

    ctx = {
      herdr,
      launch,
      params,
      sessionName,
      paneShell: params.paneShell ?? defaultPaneShell(),
      remainingMs: () => timebox.remainingMs(),
      dir(name) {
        const d = join(scratch, name);
        mkdirSync(d, { recursive: true });
        return d;
      },
      quoteForPane(argv) {
        return quoteCommand(argv, ctx.paneShell);
      },
      // format 'text' (pane reads) or 'jsonl' (wire transcripts); both are redacted fail-closed.
      capture(name, text, { format = 'text' } = {}) {
        if (!/^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(name)) throw new DriverError(`bad capture name ${JSON.stringify(name)}`);
        if (!['text', 'jsonl'].includes(format)) throw new DriverError(`bad capture format ${JSON.stringify(format)}`);
        writeFileSync(join(scratch, 'captures', name), text);
        captures.push({ name, text, format });
      },
      // A run-specific value to redact everywhere (an installation id, a session id):
      // replaced by `placeholder` in every capture and in the manifest. Only the
      // placeholder is recorded.
      redactLiteral(value, placeholder) {
        const lit = parseLiteralSpec(`${value}=${placeholder}`);
        extraLiterals.push(lit);
      },
      record(key, value) {
        manifest.scenarioData[key] = value;
      },
      // `<harness> --version` as recorded in the manifest before the scenario started
      // (first line, or an `N/A (...)` string), for the harnesses the scenario declares.
      harnessVersion(name) {
        const v = manifest.harnessVersions?.[name];
        return typeof v === 'string' ? v : null;
      },
      recordLaunchArgv(argv) {
        manifest.launch.herdrReportedArgv = argv;
      },
      // Start the scenario's launch command as a herdr agent in paneId: launch[0] is the
      // herdr agent kind, the rest pass through after `--`. herdr's reported argv is recorded
      // beside the verbatim launch parameter.
      async startAgent(name, { paneId, timeoutMs, allowErrorCodes = [] }) {
        const started = await herdr.agentStart(name, { launchArgv: launch, paneId, timeoutMs, allowErrorCodes });
        manifest.launch.herdrReportedArgv = started.argv;
        return started;
      },
      finding(text) {
        manifest.findings.push(text);
      },
      // A process outside the driver's control (e.g. the operator's shared Codex app-server
      // daemon, which the driver never stops) may keep a handle under scratch past the run.
      // Recorded; named in the finding if scratch removal then fails (#202).
      noteScratchHolder(text) {
        if (!manifest.scratch.holders.includes(text)) manifest.scratch.holders.push(text);
      },
      async probeEnv(paneId, { timeoutMs = 20000 } = {}) {
        const nonce = randomBytes(4).toString('hex');
        const out = join(scratch, `env-probe-${nonce}.json`);
        await herdr.paneRun(paneId, ctx.quoteForPane([process.execPath, PROBE_PATH, out, nonce]));
        await herdr.paneWaitOutput(paneId, { regex: `^\\s*ENVPROBE-OK-${nonce}\\s*$`, timeoutMs });
        const delta = paneEnvDelta(Object.keys(herdrEnv), JSON.parse(readFileSync(out, 'utf8')));
        manifest.env.pane = { paneId, ...delta };
        return delta;
      },
    };
  } catch (err) {
    // Never let a cleanup failure mask the setup error; a leftover is named on the console.
    const rm = await removeScratch(scratch);
    if (!rm.removed) console.error(`herdr driver: scratch directory left behind after a setup error (${rm.errors.at(-1)?.code ?? 'error'}): ${basename(scratch)} in the OS temp directory`);
    throw err;
  }

  // First signal: stop the run (settles the race below, kills in-flight herdr calls) and
  // tear down normally. Second: warn. Third: force exit, leaving the session behind.
  let signals = 0;
  const onSignal = () => {
    signals += 1;
    if (signals === 1) abort.abort('operator abort (signal received)');
    else if (signals === 2) console.error('herdr driver: teardown in progress; signal again to force exit (leaves the herdr session and scratch behind)');
    else {
      killTree(herdr.server?.child.pid);
      process.exit(130);
    }
  };
  process.on('SIGINT', onSignal);
  process.on('SIGTERM', onSignal);

  // The timebox (plus grace for in-flight bounded commands) and an operator abort each end
  // the run, whatever the scenario is awaiting -- herdr or not.
  let boxTimer;
  const stopped = new Promise((_, reject) => {
    boxTimer = setTimeout(() => abort.abort(`timebox expired (${timeboxMs} ms budget)`), timeboxMs + graceMs);
    abort.signal.addEventListener('abort', () => reject(new NotRunError(herdr.abortReason())), { once: true });
  });
  stopped.catch(() => {});

  let serverStarted = false;
  let scenarioEnd = null;
  const body = async () => {
    // Which herdr executable runs (#140): recorded first, whatever the run's outcome.
    manifest.herdr.executable = await herdrIdentity(herdrResolved, { env: herdrEnv });
    if (!herdrResolved.path) throw new NotRunError('herdr executable not resolved (on PATH, absolute entries only, or --herdr-bin); nothing spawned');
    // The pin comes from PINS.md as committed at HEAD, never the working tree. An uncommitted
    // edit to the herdr row refuses the run; any other uncommitted PINS.md edit is a finding
    // only (#139; harness versions are never gated, #216).
    let pin;
    try {
      const committed = readCommittedHerdrPin(REPO_ROOT);
      pin = committed.pin;
      manifest.herdr.pinsSource = committed.source;
      if (committed.finding) manifest.findings.push(committed.finding);
    } catch (err) {
      if (err.source) manifest.herdr.pinsSource = err.source;
      throw new NotRunError(`cannot read the herdr pin: ${err.message}. Refusing to run.`);
    }
    manifest.herdr.pinnedTag = pin.tag;
    manifest.herdr.expectedVersionOutput = pin.expectedVersionOutput;
    let observed;
    try {
      observed = await herdr.version();
    } catch (err) {
      if (err instanceof NotRunError) throw err;
      throw new NotRunError(`herdr --version could not be checked: ${err.message}`);
    }
    manifest.herdr.observedVersionOutput = observed;
    if (!versionMatches(pin, observed)) {
      throw new NotRunError(`herdr --version printed ${JSON.stringify(observed)}; PINS.md "${PIN_ROW}" requires ${JSON.stringify(pin.expectedVersionOutput)}. Refusing to run.`);
    }

    const harnesses = scenario.harnesses ?? [];
    if (harnesses.length) {
      const probe = await probeHarnesses(harnesses);
      manifest.harnessVersions = probe.versions;
      manifest.harnessExecutables = probe.executables;
    } else {
      manifest.harnessVersions = { note: 'N/A: this scenario launches no harness' };
      manifest.harnessExecutables = { note: 'N/A: this scenario launches no harness' };
    }

    if (abort.signal.aborted) throw new NotRunError(herdr.abortReason());
    serverStarted = true;
    manifest.herdr.serverStatus = await herdr.startServer({ logPath: join(scratch, 'server.log'), expectVersion: pin.version });
    manifest.session.serverPid = herdr.server.child.pid ?? null;
    try {
      manifest.herdr.agentManifests = await herdr.agentManifests();
    } catch (err) {
      if (err instanceof NotRunError) throw err;
      manifest.findings.push(`server agent-manifests not recorded: ${err.message}`);
    }

    await scenario.run(ctx);
  };

  state.phase = 'run';
  const bodyPromise = body();
  bodyPromise.catch(() => {});
  try {
    await Promise.race([bodyPromise, stopped]);
    // A scenario that caught a timeout itself, or outlived its timebox, did not run as
    // specified: NOT RUN, whatever it returned.
    const timedOut = commands.find((c) => c.timedOut);
    if (herdr.inputHalted || timedOut || timebox.remainingMs() < 0) {
      throw new NotRunError(
        `scenario returned normally, but ${timedOut ? `command #${timedOut.seq} (${timedOut.role}) timed out` : herdr.inputHalted ? `input was halted (${herdr.inputHalted})` : 'the timebox expired'}; a run with a timeout is NOT RUN`,
      );
    }
    manifest.outcome = 'PASS';
  } catch (err) {
    if (err instanceof NotRunError) {
      manifest.outcome = 'NOT RUN';
      manifest.outcomeReason = err.reason;
    } else {
      manifest.outcome = 'FAIL';
      manifest.outcomeReason = err instanceof DriverError ? err.message : `${err.name}: ${err.message}`;
    }
  } finally {
    state.phase = 'teardown';
    clearTimeout(boxTimer);
    scenarioEnd = Date.now();
    // Same rule for a scenario that failed after a timeout: the run is NOT RUN.
    const timedOut = commands.find((c) => c.timedOut);
    if (manifest.outcome === 'FAIL' && (timedOut || herdr.inputHalted || scenarioEnd - timeboxStart > timeboxMs)) {
      manifest.findings.push(`scenario failed after a timeout or timebox expiry (${manifest.outcomeReason}); recorded as NOT RUN`);
      manifest.outcome = 'NOT RUN';
      manifest.outcomeReason = timedOut ? `command #${timedOut.seq} (${timedOut.role}) timed out` : herdr.inputHalted ?? 'timebox expired';
    }
    // Make sure scenario code has stopped issuing commands before teardown; every herdr
    // call now refuses (aborted), so this settles quickly.
    if (!abort.signal.aborted) abort.abort('run over; tearing down');
    await within(bodyPromise.catch(() => {}), 2000);

    if (serverStarted) {
      try {
        manifest.teardown = await herdr.teardown();
      } catch (err) {
        manifest.teardown = { clean: false, error: err.message };
      }
    } else {
      manifest.teardown = { clean: true, note: 'no herdr session was started' };
    }
    manifest.session.panePids = [...herdr.panePids.keys()];
    // #140: the herdr file hashed at run start must still hash the same after the run; a
    // change (the file replaced mid-run) is a finding and the recorded hash is not the one
    // that necessarily ran.
    if (manifest.herdr.executable?.sha256) {
      const after = await herdrIdentity(herdrResolved, { env: herdrEnv });
      manifest.herdr.executable.unchangedAfterRun = after.sha256 === manifest.herdr.executable.sha256;
      if (!manifest.herdr.executable.unchangedAfterRun) manifest.findings.push('the herdr executable changed during the run (its sha256 at teardown differs from run start); herdr.executable.sha256 is not necessarily the file that ran');
    }
    Object.assign(manifest.timebox, {
      end: iso(scenarioEnd),
      elapsedMs: scenarioEnd - timeboxStart,
      expired: scenarioEnd - timeboxStart > timeboxMs,
      teardownEnd: iso(Date.now()),
    });

    manifest.harnessConfig.after = hashHarnessConfig();
    Object.assign(manifest.harnessConfig, compareHashes(manifest.harnessConfig.before, manifest.harnessConfig.after));
    if (!manifest.harnessConfig.unchanged) {
      manifest.findings.push(`harness config hash changed during the run: ${manifest.harnessConfig.changed.join(', ')} (attribute before blaming herdr: the harness may write its own settings; K1 §6 L8)`);
    }
    const downgrade = (reason) => {
      manifest.findings.push(reason);
      if (manifest.outcome === 'PASS') {
        manifest.outcome = 'FAIL';
        manifest.outcomeReason = reason;
      }
    };
    if (!manifest.teardown.clean) downgrade(`teardown was not clean: ${JSON.stringify({ ...manifest.teardown, clean: undefined })}`);

    // Redaction literals first: realpath needs the scratch dir to still exist.
    const literals = [{ value: scratch, placeholder: '<SCRATCH>' }, { value: REPO_ROOT, placeholder: '<REPO>' }, { value: outDir, placeholder: '<OUT>' }, ...extraLiterals];
    for (const { value, placeholder } of [...literals]) {
      try {
        const real = realpathSync(value);
        if (real !== value) literals.push({ value: real, placeholder });
      } catch {
        /* not a path, or not resolvable */
      }
    }
    manifest.redaction.runLiteralPlaceholders = [...new Set(extraLiterals.map((l) => l.placeholder))];
    // Scratch removal never aborts the run record (#202): it retries with a bounded backoff,
    // and a failure that outlasts the retries is recorded (teardown.clean=false, the leftover
    // path redacted like every other path, a finding) and does not change the outcome -- the
    // run itself was unaffected, and the leftover is outside the repository, as with
    // --keep-scratch. The raw path is printed on the console for the operator to delete.
    if (opts.keepScratch) manifest.scratch.removed = false;
    else {
      const rm = await removeScratch(scratch);
      manifest.scratch.removed = rm.removed;
      manifest.scratch.removal = { attempts: rm.attempts, errors: rm.errors };
      if (rm.errors.length && rm.removed) {
        manifest.findings.push(`scratch removal succeeded on attempt ${rm.attempts} after ${rm.errors.map((e) => e.code ?? 'error').join(', ')}`);
      }
      if (!rm.removed) {
        const last = rm.errors.at(-1);
        manifest.teardown = { ...manifest.teardown, clean: false, leftover: scratch, leftoverError: last?.message ?? null };
        const holders = manifest.scratch.holders.length ? ` Possible holder(s) the scenario declared: ${manifest.scratch.holders.join(' | ')}` : '';
        manifest.findings.push(`scratch directory could not be removed after ${rm.attempts} attempts (${last?.code ?? 'error'}); left behind at ${scratch} -- unredacted captures may remain there; delete it by hand once released.${holders}`);
        leftover = scratch;
      }
    }

    state.phase = 'record';

    const redactor = createRedactor({ literals });
    for (const c of captures) {
      const { text, report } = c.format === 'jsonl' ? redactor.redactJsonl(c.text) : redactor.redactText(c.text);
      const ok = reportIsClean(report);
      if (ok) writeFileSync(join(outDir, c.name), text);
      // sha256 of the bytes written (#140): binds a committed fixture to this capture.
      manifest.captures.push({ file: c.name, format: c.format, written: ok, sha256: ok ? sha256Text(text) : null, redaction: report });
      if (!ok) downgrade(`capture ${c.name} still carried residual hits or hazard protocol frames after redaction and was withheld (${summarize(report)})`);
    }
    // The manifest is always written, but never with a residual hit in it: a value that
    // redaction could not prove clean is replaced by <WITHHELD: ...> and listed by path.
    const red = redactor.redactValue(manifest);
    let written = red.value;
    let withheld = [];
    if (!reportIsClean(red.report)) {
      ({ value: written, withheld } = redactor.withholdResiduals(red.value));
      written.findings.push(`run-manifest values withheld because redaction left residual hits: ${withheld.map((w) => `${w.path} [${w.labels.join(', ')}]`).join('; ')}`);
    }
    const finalScan = redactor.scan(JSON.stringify(written, null, 2));
    written.manifestRedaction = {
      afterRedaction: summarize(red.report),
      replacements: red.report.replacements,
      withheld,
      writtenClean: finalScan.residualLeaks.length === 0 && finalScan.residualGenericHits.length === 0,
    };
    process.off('SIGINT', onSignal);
    process.off('SIGTERM', onSignal);
    if (!written.manifestRedaction.writtenClean) throw new DriverError('run manifest could not be made clean of residual hits; not written');
    writeFileSync(join(outDir, 'run-manifest.json'), `${JSON.stringify(written, null, 2)}\n`);
  }

  return { outcome: manifest.outcome, reason: manifest.outcomeReason, exitCode: EXIT[manifest.outcome], outDir, manifestPath: join(outDir, 'run-manifest.json'), manifest, scratchLeftover: leftover };
}

async function main(argv) {
  let opts;
  try {
    opts = parseArgs(argv);
  } catch (err) {
    console.error(`${err.message}\nusage: node tools/herdr/run.mjs --scenario <name|path> [--launch '<json argv>'] [--param k=v] [--timebox-ms N] [--out DIR] [--keep-scratch] [--redact-literal V=<P>] [--herdr-bin PATH] | --self-test`);
    return EXIT.USAGE;
  }
  if (opts.help) {
    console.log(readFileSync(fileURLToPath(import.meta.url), 'utf8').split('\n').filter((l) => l.startsWith('//')).map((l) => l.slice(3)).join('\n'));
    return 0;
  }
  if (opts.selfTest) {
    const { runSelfTest } = await import('./test/selftest.mjs');
    return runSelfTest();
  }
  let res;
  try {
    res = await runScenario(opts);
  } catch (err) {
    console.error(err instanceof UsageError ? err.message : `driver error (${err?.phase ?? 'unknown'} phase): ${err?.message ?? err}`);
    return err instanceof UsageError ? EXIT.USAGE : EXIT.FAIL;
  }
  console.log(`scenario: ${res.manifest.scenario.name}`);
  console.log(`outcome:  ${res.outcome}${res.reason ? ` -- ${res.reason}` : ''}`);
  console.log(`teardown: ${res.manifest.teardown?.clean ? 'clean' : 'NOT clean'}`);
  if (res.scratchLeftover) console.log(`scratch left behind (delete by hand): ${res.scratchLeftover}`);
  console.log(`manifest: ${res.manifestPath}`);
  return res.exitCode;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const code = await main(process.argv.slice(2));
  // Exit explicitly once the run is recorded: a scenario abandoned at its timebox or on an
  // operator abort may still hold timers or handles that would keep the process alive.
  process.stdout.write('', () => process.stderr.write('', () => process.exit(code)));
}
