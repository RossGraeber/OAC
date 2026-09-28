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
//   --herdr-bin <path>       herdr executable (default: `herdr` on PATH; a .mjs path runs
//                            under node -- used by the self-test's fake herdr)
//
// Outcome and exit code: PASS 0, FAIL 1, usage error 2, NOT RUN 3. A timeout, an expired
// timebox, an operator abort, or a herdr version other than the PINS.md pin is NOT RUN --
// never a failure and never a fabricated pass.
//
// Test tooling only. Node built-ins only; no package.json. herdr is an external process,
// never linked. The driver never reads harness credentials, never writes harness config
// (it hashes it before and after), and never runs the herdr subcommand that adds hooks
// to a harness's config.
// Scope and boundaries: docs/planning/decisions/K1-herdr-evaluation.md,
// scripts/check-herdr-containment.mjs (oac-boundaries checks 9 and 10).

import { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, isAbsolute, join, relative, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { randomBytes } from 'node:crypto';

import { readHerdrPin, versionMatches, PIN_ROW } from './lib/pins.mjs';
import { HerdrSession, NotRunError, DriverError, makeSessionName } from './lib/herdr.mjs';
import {
  MANIFEST_SCHEMA_VERSION, HERDR_RUN_CONFIG, driverInfo, osInfo, hashHarnessConfig, compareHashes,
  harnessVersions, herdrLaunchEnv, paneEnvDelta,
} from './lib/manifest.mjs';
import { createRedactor, reportIsClean, summarize } from './lib/redact.mjs';
import { defaultPaneShell, quoteCommand } from './lib/pane-shell.mjs';
import { killTree } from './lib/proc.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
export const REPO_ROOT = resolve(HERE, '..', '..');
const PINS_PATH = join(REPO_ROOT, 'docs', 'planning', 'PINS.md');
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

export async function runScenario(opts) {
  const scenario = await loadScenario(opts.scenario);
  const launch = opts.launch ?? scenario.defaults?.launch ?? [];
  const params = { ...(scenario.defaults?.params ?? {}), ...opts.params };
  const timeboxMs = opts.timeboxMs ?? scenario.defaults?.timeboxMs ?? 300000;
  const runId = `${new Date().toISOString().replace(/[-:]/g, '').replace(/\.\d+Z$/, 'Z')}-${randomBytes(3).toString('hex')}`;

  // Scratch lives outside the repository, always; raw captures never land in the tree.
  const scratch = mkdtempSync(join(tmpdir(), 'oac-herdr-scratch-'));
  if (isInside(scratch, REPO_ROOT)) throw new DriverError('scratch directory resolved inside the repository; refusing to run');
  mkdirSync(join(scratch, 'captures'));
  const outDir = resolve(opts.out ?? join(tmpdir(), 'oac-herdr-runs', runId));
  mkdirSync(outDir, { recursive: true });

  const sessionName = makeSessionName(scenario.name);
  const timeboxStart = Date.now();
  const timebox = { remainingMs: () => timeboxStart + timeboxMs - Date.now() };
  const commands = [];
  const abort = new AbortController();

  const configPath = join(scratch, 'herdr-config.toml');
  writeFileSync(configPath, HERDR_RUN_CONFIG);
  const { env: herdrEnv, delta: serverEnvDelta } = herdrLaunchEnv(process.env, configPath);
  const herdr = new HerdrSession({
    herdrCmd: herdrCommand(opts.herdrBin),
    sessionName,
    env: herdrEnv,
    cwd: scratch,
    timebox,
    commands,
    abortSignal: abort.signal,
  });

  const manifest = {
    schemaVersion: MANIFEST_SCHEMA_VERSION,
    runId,
    outcome: null,
    outcomeReason: null,
    scenario: { name: scenario.name, file: scenario.file, description: scenario.description ?? null, params },
    launch: { argv: launch, herdrReportedArgv: null },
    driver: driverInfo(REPO_ROOT),
    herdr: {
      pinRow: PIN_ROW,
      pinnedTag: null,
      expectedVersionOutput: null,
      observedVersionOutput: null,
      serverStatus: null,
      config: { path: configPath, contents: HERDR_RUN_CONFIG },
      agentManifests: null,
    },
    harnessVersions: {},
    os: osInfo(),
    session: { name: sessionName, serverPid: null, panePids: [] },
    env: { serverLaunch: serverEnvDelta, pane: null },
    harnessConfig: { before: hashHarnessConfig(), after: null, unchanged: null, changed: [] },
    timebox: { budgetMs: timeboxMs, start: iso(timeboxStart), end: null, elapsedMs: null, expired: null },
    commands,
    teardown: null,
    scratch: { location: 'os.tmpdir(), outside the repository', path: scratch, removed: null },
    captures: [],
    scenarioData: {},
    findings: [],
  };

  const captures = [];
  const ctx = {
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
    capture(name, text) {
      if (!/^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(name)) throw new DriverError(`bad capture name ${JSON.stringify(name)}`);
      writeFileSync(join(scratch, 'captures', name), text);
      captures.push({ name, text });
    },
    record(key, value) {
      manifest.scenarioData[key] = value;
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

  let signals = 0;
  const onSignal = () => {
    signals += 1;
    if (signals > 1) {
      killTree(herdr.server?.child.pid);
      process.exit(130);
    }
    abort.abort();
  };
  process.on('SIGINT', onSignal);
  process.on('SIGTERM', onSignal);

  let serverStarted = false;
  try {
    let pin;
    try {
      pin = readHerdrPin(PINS_PATH);
    } catch (err) {
      throw new NotRunError(`cannot read the herdr pin: ${err.message}`);
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
    manifest.harnessVersions = harnesses.length ? await harnessVersions(harnesses) : { note: 'N/A: this scenario launches no harness' };

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
    process.off('SIGINT', onSignal);
    process.off('SIGTERM', onSignal);
    if (serverStarted) {
      try {
        manifest.teardown = await herdr.teardown();
      } catch (err) {
        manifest.teardown = { clean: false, error: err.message };
      }
    } else {
      manifest.teardown = { clean: true, note: 'no herdr session was started' };
    }
    manifest.session.panePids = [...herdr.panePids];
    const timeboxEnd = Date.now();
    Object.assign(manifest.timebox, {
      end: iso(timeboxEnd),
      elapsedMs: timeboxEnd - timeboxStart,
      expired: timeboxEnd - timeboxStart > timeboxMs,
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
    const literals = [{ value: scratch, placeholder: '<SCRATCH>' }, { value: REPO_ROOT, placeholder: '<REPO>' }, { value: outDir, placeholder: '<OUT>' }];
    for (const { value, placeholder } of [...literals]) {
      try {
        const real = realpathSync(value);
        if (real !== value) literals.push({ value: real, placeholder });
      } catch {
        /* not resolvable */
      }
    }
    if (!opts.keepScratch) rmSync(scratch, { recursive: true, force: true });
    manifest.scratch.removed = !opts.keepScratch;

    const redactor = createRedactor({ literals });
    for (const c of captures) {
      const { text, report } = redactor.redactText(c.text);
      const ok = reportIsClean(report);
      if (ok) writeFileSync(join(outDir, c.name), text);
      manifest.captures.push({ file: c.name, written: ok, redaction: report });
      if (!ok) downgrade(`capture ${c.name} still carried residual hits after redaction and was withheld (${summarize(report)})`);
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
    if (!written.manifestRedaction.writtenClean) throw new DriverError('run manifest could not be made clean of residual hits; not written');
    writeFileSync(join(outDir, 'run-manifest.json'), `${JSON.stringify(written, null, 2)}\n`);
  }

  return { outcome: manifest.outcome, reason: manifest.outcomeReason, exitCode: EXIT[manifest.outcome], outDir, manifestPath: join(outDir, 'run-manifest.json'), manifest };
}

async function main(argv) {
  let opts;
  try {
    opts = parseArgs(argv);
  } catch (err) {
    console.error(`${err.message}\nusage: node tools/herdr/run.mjs --scenario <name|path> [--launch '<json argv>'] [--param k=v] [--timebox-ms N] [--out DIR] [--keep-scratch] [--herdr-bin PATH] | --self-test`);
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
    console.error(err instanceof UsageError ? err.message : `driver error: ${err.message}`);
    return err instanceof UsageError ? EXIT.USAGE : EXIT.FAIL;
  }
  console.log(`scenario: ${res.manifest.scenario.name}`);
  console.log(`outcome:  ${res.outcome}${res.reason ? ` -- ${res.reason}` : ''}`);
  console.log(`teardown: ${res.manifest.teardown?.clean ? 'clean' : 'NOT clean'}`);
  console.log(`manifest: ${res.manifestPath}`);
  return res.exitCode;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  process.exitCode = await main(process.argv.slice(2));
}
