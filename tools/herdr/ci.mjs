#!/usr/bin/env node
// CI entry point for the opt-in herdr workflow (Epic K, K6 #129). The workflow
// .github/workflows/herdr-provider-optin.yml calls this file and nothing else under
// tools/herdr/. It runs one allowlisted scenario on an operator-owned self-hosted runner,
// checks that the evidence is clean, and stages only the redacted files for upload.
//
//   node tools/herdr/ci.mjs run       # run the scenario named by OAC_HERDR_SCENARIO
//   node tools/herdr/ci.mjs stage     # check the run's evidence; copy allowlisted files to <workdir>/upload
//   node tools/herdr/ci.mjs cleanup   # delete <workdir>; fail if the checkout was left dirty
//   option: --workdir <dir>           # default: $RUNNER_TEMP/oac-herdr-<run id>-<attempt>
//
// Inputs come from the environment, never from text the workflow splices into a command:
//   OAC_HERDR_SCENARIO   the workflow_dispatch `scenario` input; empty on a push run, which
//                        means `smoke`. Only names in CI_SCENARIOS are accepted. A path,
//                        a flag, or any other name is a usage error.
//
// What `run` does not do, by design:
//   - it passes no --param, --launch, --herdr-bin or --keep-scratch to the driver, so every
//     scenario runs on its own defaults. Since the #196 operator decision g1-claude-wake
//     defaults to accept=driver, so a CI G1 run is unattended: the driver accepts the three
//     dialogs on record (trust, MCP approval, dev-channels), records each as `driver`, and
//     refuses anything else (NOT RUN). G1 criterion 5 is then `not evaluable`, so a CI G1
//     run is never a G1 equivalence record (oac-gates references/scripted-runs.md,
//     "Operator-consent dialogs"). CI cannot switch to accept=human either;
//   - it never re-runs the driver after a FAIL or NOT RUN (scripted-runs.md, "No automatic
//     re-submission");
//   - it never reads, copies or uploads anything from the harness's own home or config.
//     Harness login is the runner operator's, done once by hand outside CI
//     (docs/planning/gates/herdr-runner.md).
// It removes CI job tokens (every ACTIONS_* variable, GITHUB_TOKEN, GH_TOKEN) and the
// workflow-command file paths (GITHUB_ENV, GITHUB_OUTPUT, GITHUB_PATH, GITHUB_STATE,
// GITHUB_STEP_SUMMARY) from the environment before the driver starts, so neither herdr nor
// the harness under test is handed them. (Defense in depth only: the harness runs as the
// runner's own user; see the threat table in docs/planning/gates/herdr-runner.md.) It points the driver's temp directory (TMPDIR/TEMP/TMP) into <workdir>/tmp, so
// the run's scratch directory lives under the job's own temp directory and goes with it.
//
// `stage` uploads nothing itself. It stages files only when ALL of these hold, else it
// stages nothing and exits 1:
//   - <workdir>/out/run-manifest.json exists as a regular file, its
//     manifestRedaction.writtenClean is true, and its teardown.clean is true (a run whose
//     teardown was not clean may have left a process that could still rewrite files);
//   - every capture the manifest lists was written (none withheld by the driver), has a
//     name matching CAPTURE_NAME, exists as a regular file, and its recorded redaction
//     report is clean (no residualLeaks, residualGenericHits or hazardProtocolFrames);
//   - an independent re-scan of every file to be staged, with this machine's home path,
//     user name and host name plus the checkout and work-dir paths, finds no residual hit.
// Each file is read once: the bytes that were scanned are the bytes written to upload/.
// Only run-manifest.json and the listed captures are staged. Anything else under out/ is
// left behind and deleted by `cleanup`.
//
// Exit codes: run = the driver's (PASS 0, FAIL 1, usage 2, NOT RUN 3); stage and cleanup
// 0 ok, 1 not clean, 2 usage error.

import { existsSync, lstatSync, mkdirSync, readdirSync, readFileSync, realpathSync, rmSync, appendFileSync, writeFileSync } from 'node:fs';
import { dirname, isAbsolute, join, relative, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath, pathToFileURL } from 'node:url';

import { createRedactor, reportIsClean, summarize } from './lib/redact.mjs';

// run.mjs is imported lazily (in ciRun): the self-test reaches this file from inside
// run.mjs's own top-level await, and a static import back into run.mjs would deadlock.
export const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
const USAGE_EXIT = 2;

// The scenarios CI may run. Each runs on its own defaults; nothing else is passed.
export const CI_SCENARIOS = Object.freeze(['smoke', 'g1-claude-wake']);
export const PUSH_SCENARIO = 'smoke';

// Captures the driver's scenarios write: pane reads and wire transcripts.
export const CAPTURE_NAME = /^(?:pane|transcript)-[A-Za-z0-9][A-Za-z0-9._-]*\.(?:txt|jsonl)$/;
export const MANIFEST_NAME = 'run-manifest.json';

// CI job tokens and workflow-command files: never handed to herdr or to the harness under test.
const CI_TOKEN_ENV = /^(?:ACTIONS_.*|GITHUB_TOKEN|GH_TOKEN|GH_ENTERPRISE_TOKEN|GITHUB_(?:ENV|OUTPUT|PATH|STATE|STEP_SUMMARY))$/i;
// ...and any variable, whatever its name, whose value looks like a GitHub token (for
// example a job token copied into a custom env: name).
const GITHUB_TOKEN_VALUE = /\b(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,})/;

class UsageError extends Error {}

export function resolveScenario(value) {
  const name = value === undefined || value === '' ? PUSH_SCENARIO : value;
  if (!CI_SCENARIOS.includes(name)) {
    throw new UsageError(`scenario ${JSON.stringify(String(name).slice(0, 80))} is not one CI may run (allowed: ${CI_SCENARIOS.join(', ')})`);
  }
  return name;
}

export function scrubEnv(env) {
  const removed = [];
  for (const key of Object.keys(env)) {
    if (CI_TOKEN_ENV.test(key) || GITHUB_TOKEN_VALUE.test(String(env[key] ?? ''))) {
      delete env[key];
      removed.push(key);
    }
  }
  return removed.sort();
}

export function defaultWorkdir(env = process.env) {
  const { RUNNER_TEMP: temp, GITHUB_RUN_ID: id, GITHUB_RUN_ATTEMPT: attempt } = env;
  if (!temp || !isAbsolute(temp)) throw new UsageError('RUNNER_TEMP is not set to an absolute path; pass --workdir outside CI');
  if (!/^\d+$/.test(id ?? '') || !/^\d+$/.test(attempt ?? '')) throw new UsageError('GITHUB_RUN_ID / GITHUB_RUN_ATTEMPT are not numeric; pass --workdir outside CI');
  return join(temp, `oac-herdr-${id}-${attempt}`);
}

function insideRepo(dir) {
  let root = REPO_ROOT;
  try {
    root = realpathSync(REPO_ROOT);
  } catch {
    /* keep as is */
  }
  let d = resolve(dir);
  try {
    d = realpathSync(d);
  } catch {
    /* not created yet */
  }
  const rel = relative(root, d);
  return rel === '' || (!rel.startsWith('..') && !isAbsolute(rel));
}

function checkWorkdir(workdir) {
  if (!isAbsolute(workdir)) throw new UsageError('the work dir must be an absolute path');
  if (insideRepo(workdir)) throw new UsageError('the work dir must be outside the repository checkout');
}

const paths = (workdir) => ({ tmp: join(workdir, 'tmp'), out: join(workdir, 'out'), upload: join(workdir, 'upload') });

// --- run ---------------------------------------------------------------------------------

export async function ciRun({ workdir, scenario }) {
  checkWorkdir(workdir);
  const name = resolveScenario(scenario);
  if (existsSync(workdir)) throw new UsageError('the work dir already exists; refusing to reuse a stale run directory');
  const p = paths(workdir);
  mkdirSync(p.tmp, { recursive: true });
  mkdirSync(p.out, { recursive: true });

  const removed = scrubEnv(process.env);
  if (removed.length) console.log(`ci: removed from the driver's environment: ${removed.join(', ')}`);
  for (const k of ['TMPDIR', 'TEMP', 'TMP']) process.env[k] = p.tmp;

  const { parseArgs, runScenario } = await import('./run.mjs');
  const argv = ['--scenario', name, '--out', p.out];
  const opts = parseArgs(argv);
  console.log(`ci: scenario ${name}, driver defaults only (no --param, --launch or --keep-scratch)`);
  const res = await runScenario(opts);
  // Job logs of a public repository are public: print the outcome from the redacted
  // manifest the driver wrote, never from the in-memory (unredacted) record.
  let written = null;
  try {
    written = JSON.parse(readFileSync(join(p.out, MANIFEST_NAME), 'utf8'));
  } catch {
    /* reported by stage */
  }
  console.log(`scenario: ${name}`);
  console.log(`outcome:  ${res.outcome}${written?.outcomeReason ? ` -- ${written.outcomeReason}` : ''}`);
  console.log(`teardown: ${written?.teardown?.clean ? 'clean' : 'NOT clean (or no manifest written)'}`);
  return res.exitCode;
}

// --- stage -------------------------------------------------------------------------------

function regularFile(path) {
  try {
    return lstatSync(path).isFile();
  } catch {
    return false;
  }
}

const safeName = (n) => (/^[A-Za-z0-9._-]{1,120}$/.test(n) ? n : '<unprintable name>');

// Returns { ok, problems, files, upload, manifest }. Stages nothing unless ok.
export function ciStage({ workdir }) {
  checkWorkdir(workdir);
  const p = paths(workdir);
  const problems = [];
  const fail = (msg) => ({ ok: false, problems: [...problems, msg], files: [], upload: null, manifest: null });

  const manifestPath = join(p.out, MANIFEST_NAME);
  if (!regularFile(manifestPath)) return fail(`${MANIFEST_NAME} is missing or not a regular file (the driver did not finish writing its record)`);
  let manifest;
  try {
    manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
  } catch {
    return fail(`${MANIFEST_NAME} is not valid JSON`);
  }
  if (manifest?.manifestRedaction?.writtenClean !== true) problems.push(`${MANIFEST_NAME}: manifestRedaction.writtenClean is not true`);
  if (manifest?.teardown?.clean !== true) problems.push(`${MANIFEST_NAME}: teardown was not clean (a surviving process could still change the evidence)`);

  const files = [MANIFEST_NAME];
  const captures = Array.isArray(manifest?.captures) ? manifest.captures : null;
  if (!captures) problems.push(`${MANIFEST_NAME}: no captures list`);
  for (const c of captures ?? []) {
    const file = typeof c?.file === 'string' ? c.file : '';
    if (!CAPTURE_NAME.test(file)) {
      problems.push(`capture ${safeName(file)}: name is not an allowlisted capture name`);
      continue;
    }
    if (c.written !== true) {
      problems.push(`capture ${file}: withheld by the driver (redaction could not make it clean)`);
      continue;
    }
    const r = c.redaction;
    const clean = r && Array.isArray(r.residualLeaks) && Array.isArray(r.residualGenericHits) && Array.isArray(r.hazardProtocolFrames) && reportIsClean(r);
    if (!clean) problems.push(`capture ${file}: recorded redaction report is not clean${r?.residualLeaks ? ` (${summarize({ droppedHazardLines: r.droppedHazardLines ?? 0, hazardProtocolFrames: r.hazardProtocolFrames ?? [], residualLeaks: r.residualLeaks ?? [], residualGenericHits: r.residualGenericHits ?? [] })})` : ''}`);
    if (!regularFile(join(p.out, file))) problems.push(`capture ${file}: missing or not a regular file`);
    files.push(file);
  }
  if (new Set(files).size !== files.length) problems.push('a capture name is listed twice');

  // Independent re-scan with this machine's identity and this job's paths. The driver
  // already scanned; this is the gate the upload depends on.
  const literals = [
    { value: REPO_ROOT, placeholder: '<REPO>' },
    { value: resolve(workdir), placeholder: '<WORKDIR>' },
  ];
  for (const l of [...literals]) {
    try {
      const real = realpathSync(l.value);
      if (real !== l.value) literals.push({ value: real, placeholder: l.placeholder });
    } catch {
      /* not resolvable */
    }
  }
  const scanner = createRedactor({ literals });
  const scanned = new Map(); // file -> the exact bytes scanned; only these are staged
  for (const f of files) {
    if (!regularFile(join(p.out, f))) continue;
    const bytes = readFileSync(join(p.out, f));
    scanned.set(f, bytes);
    const { residualLeaks, residualGenericHits } = scanner.scan(bytes.toString('utf8'));
    if (residualLeaks.length || residualGenericHits.length) {
      const labels = [...new Set([...residualLeaks, ...residualGenericHits].map((h) => h.label))].join(', ');
      problems.push(`${f}: re-scan found ${residualLeaks.length} residual leak(s) and ${residualGenericHits.length} generic hit(s) [${labels}]`);
    }
  }

  const ignored = readdirSync(p.out).filter((n) => !files.includes(n));
  if (problems.length) return { ok: false, problems, files: [], upload: null, manifest, ignored };

  if (existsSync(p.upload)) return fail('the upload directory already exists; refusing to stage into it');
  mkdirSync(p.upload);
  for (const f of files) writeFileSync(join(p.upload, f), scanned.get(f), { flag: 'wx' });
  return { ok: true, problems: [], files, upload: p.upload, manifest, ignored };
}

function writeStepSummary(res) {
  const target = process.env.GITHUB_STEP_SUMMARY;
  if (!target) return;
  const m = res.manifest ?? {};
  const lines = [
    '### herdr opt-in run',
    '',
    `- Scenario: \`${m.scenario?.name ?? 'unknown'}\``,
    `- Run outcome (driver, not a gate verdict): \`${m.outcome ?? 'unknown'}\``,
    `- Evidence: ${res.ok ? `clean; staged ${res.files.map((f) => `\`${f}\``).join(', ')}` : 'NOT clean; nothing uploaded'}`,
    ...res.problems.map((x) => `  - ${x}`),
    '',
    'Not verdict-bearing (oac-gates `references/scripted-runs.md`). A fixture from this run still needs its record\'s Verification section, re-checked by the recording agent (#252).',
    '',
  ];
  try {
    appendFileSync(target, lines.join('\n'));
  } catch {
    /* the summary is a convenience */
  }
}

export function redactMessage(message) {
  try {
    const literals = [{ value: REPO_ROOT, placeholder: '<REPO>' }];
    if (process.env.RUNNER_TEMP) literals.push({ value: process.env.RUNNER_TEMP, placeholder: '<RUNNER_TEMP>' });
    const { text, report } = createRedactor({ literals }).redactText(String(message ?? ''));
    return reportIsClean(report) ? text.trimEnd() : '<error message withheld: redaction could not make it clean>';
  } catch {
    return '<error message withheld>';
  }
}

function writeOutput(key, value) {
  const target = process.env.GITHUB_OUTPUT;
  if (!target) return;
  if (/[\r\n]/.test(value)) throw new Error(`refusing to write a multi-line ${key} output`);
  appendFileSync(target, `${key}=${value}\n`);
}

// --- cleanup -----------------------------------------------------------------------------

export function ciCleanup({ workdir }) {
  checkWorkdir(workdir);
  const problems = [];
  rmSync(workdir, { recursive: true, force: true });
  if (existsSync(workdir)) problems.push('the work dir could not be removed');
  const git = spawnSync('git', ['status', '--porcelain=v1', '--untracked-files=all', '--ignored=matching'], { cwd: REPO_ROOT, encoding: 'utf8', timeout: 30000 });
  if (git.status !== 0) problems.push('git status failed in the checkout');
  else {
    const dirty = git.stdout.split('\n').filter(Boolean);
    if (dirty.length) problems.push(`the run left ${dirty.length} changed or untracked path(s) in the checkout: ${dirty.map((l) => l.slice(3)).slice(0, 20).join(', ')}`);
  }
  return { ok: problems.length === 0, problems };
}

// --- CLI ---------------------------------------------------------------------------------

function cliArgs(argv) {
  const [cmd, ...rest] = argv;
  if (!['run', 'stage', 'cleanup'].includes(cmd)) throw new UsageError('usage: node tools/herdr/ci.mjs run|stage|cleanup [--workdir <dir>]');
  let workdir;
  for (let i = 0; i < rest.length; i++) {
    if (rest[i] === '--workdir' && rest[i + 1] !== undefined) workdir = resolve(rest[++i]);
    else throw new UsageError(`unknown argument ${safeName(rest[i])}`);
  }
  return { cmd, workdir: workdir ?? defaultWorkdir() };
}

async function main(argv) {
  let args;
  try {
    args = cliArgs(argv);
  } catch (err) {
    console.error(err.message);
    return USAGE_EXIT;
  }
  try {
    if (args.cmd === 'run') return await ciRun({ workdir: args.workdir, scenario: process.env.OAC_HERDR_SCENARIO });
    if (args.cmd === 'stage') {
      const res = ciStage({ workdir: args.workdir });
      for (const n of res.ignored ?? []) console.log(`ci: not staged (not an allowlisted evidence file): ${safeName(n)}`);
      for (const x of res.problems) console.error(`ci: ${x}`);
      writeStepSummary(res);
      if (!res.ok) {
        console.error('ci: evidence is NOT clean; nothing is staged, nothing will be uploaded');
        return 1;
      }
      console.log(`ci: evidence clean; staged ${res.files.join(', ')}`);
      writeOutput('upload-dir', res.upload);
      return 0;
    }
    const res = ciCleanup({ workdir: args.workdir });
    for (const x of res.problems) console.error(`ci: ${x}`);
    if (res.ok) console.log('ci: work dir removed; checkout clean');
    return res.ok ? 0 : 1;
  } catch (err) {
    // Job logs are public; an fs or driver error message can carry a home path or user name.
    console.error(err instanceof UsageError ? err.message : `ci error: ${redactMessage(err?.message)}`);
    return err instanceof UsageError ? USAGE_EXIT : 1;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const code = await main(process.argv.slice(2));
  // Exit explicitly, as run.mjs does: an abandoned scenario may still hold handles.
  process.stdout.write('', () => process.stderr.write('', () => process.exit(code)));
}
