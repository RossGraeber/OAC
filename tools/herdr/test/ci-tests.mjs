// Checks for tools/herdr/ci.mjs (K6 #129), the opt-in workflow's entry point. Run from
// test/selftest.mjs (`node tools/herdr/run.mjs --self-test`).
//
// ciUnit: the scenario allowlist, the environment scrub, the work-dir rules, and the
// stage gate against hand-built run directories (clean, withheld, residual, symlink, ...).
// ciLifecycle (POSIX only, like the rest of the lifecycle half): the real driver runs the
// smoke scenario against test/fake-herdr.mjs -- a test double, not herdr -- and ciStage then
// gates that real driver output. It proves the gate accepts what the driver writes and
// stages only the allowlisted files; it proves nothing about herdr or any runner.

import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';

import { CAPTURE_NAME, CI_SCENARIOS, REPO_ROOT, ciCleanup, ciStage, defaultWorkdir, redactMessage, resolveScenario, scrubEnv } from '../ci.mjs';

const throws = (fn) => {
  try {
    fn();
    return false;
  } catch {
    return true;
  }
};

const CLEAN_REPORT = { droppedHazardLines: 0, hazardProtocolFrames: [], residualLeaks: [], residualGenericHits: [] };

// A run directory shaped like the driver's output. `mutate` edits it before staging.
function fakeRun(mutate = () => {}) {
  const workdir = mkdtempSync(join(tmpdir(), 'oac-herdr-ci-test-'));
  const out = join(workdir, 'out');
  mkdirSync(out);
  const manifest = {
    scenario: { name: 'smoke' },
    outcome: 'PASS',
    teardown: { clean: true },
    captures: [
      { file: 'pane-smoke.txt', format: 'text', written: true, redaction: { ...CLEAN_REPORT } },
      { file: 'transcript-2026-09-28-2.1.283-herdr.jsonl', format: 'jsonl', written: true, redaction: { ...CLEAN_REPORT } },
    ],
    manifestRedaction: { writtenClean: true },
  };
  const files = {
    'pane-smoke.txt': 'OAC-SMOKE-READY\n',
    'transcript-2026-09-28-2.1.283-herdr.jsonl': '{"dir":"in","payload":{"jsonrpc":"2.0","id":1}}\n',
    'server.log': 'raw server log that must never be staged\n',
  };
  const ctx = { workdir, out, manifest, files };
  mutate(ctx);
  if (ctx.manifest !== undefined) writeFileSync(join(out, 'run-manifest.json'), `${JSON.stringify(ctx.manifest, null, 2)}\n`);
  for (const [n, t] of Object.entries(ctx.files)) if (t !== null) writeFileSync(join(out, n), t);
  return ctx;
}

function stageCase(check, name, mutate, expectOk) {
  const ctx = fakeRun(mutate);
  try {
    const res = ciStage({ workdir: ctx.workdir });
    const uploaded = existsSync(join(ctx.workdir, 'upload')) ? readdirSync(join(ctx.workdir, 'upload')).sort() : null;
    if (expectOk) {
      check(`ci stage: ${name}`, res.ok && JSON.stringify(uploaded) === JSON.stringify(['pane-smoke.txt', 'run-manifest.json', 'transcript-2026-09-28-2.1.283-herdr.jsonl']), `${res.problems.join('; ')} ${uploaded}`);
    } else {
      check(`ci stage: ${name}`, !res.ok && uploaded === null && res.problems.length > 0, `ok=${res.ok} uploaded=${uploaded}`);
    }
    return res;
  } finally {
    rmSync(ctx.workdir, { recursive: true, force: true });
  }
}

export function ciUnit(check) {
  // --- scenario allowlist ---
  check('ci: a push run (empty scenario input) runs smoke', resolveScenario('') === 'smoke' && resolveScenario(undefined) === 'smoke');
  check('ci: every allowlisted scenario resolves to itself', CI_SCENARIOS.every((s) => resolveScenario(s) === s));
  for (const bad of ['../../tmp/x.mjs', 'tools/herdr/test/scenarios/slow.mjs', 'smoke.mjs', 'smoke --keep-scratch', 'SMOKE', 'smoke;id', '$(id)', 'g1-claude-wake --param accept=driver']) {
    check(`ci: scenario ${JSON.stringify(bad)} is refused`, throws(() => resolveScenario(bad)));
  }

  // --- environment scrub ---
  const env = {
    PATH: '/usr/bin',
    HOME: '/home/op',
    GITHUB_RUN_ID: '1',
    GITHUB_WORKSPACE: '/w',
    ACTIONS_RUNTIME_TOKEN: 't',
    ACTIONS_ID_TOKEN_REQUEST_TOKEN: 't',
    ACTIONS_CACHE_URL: 'u',
    GITHUB_TOKEN: 't',
    GH_TOKEN: 't',
    GITHUB_ENV: '/f',
    GITHUB_OUTPUT: '/f',
    GITHUB_PATH: '/f',
    GITHUB_STATE: '/f',
    GITHUB_STEP_SUMMARY: '/f',
  };
  env.CUSTOM_NAME = `x ghs_${'A1b2C3d4E5'.repeat(3)}`;
  env.MY_PAT = `github_pat_${'Z9'.repeat(15)}`;
  const removed = scrubEnv(env);
  check('ci: scrub removes a token-shaped value under any variable name', !('CUSTOM_NAME' in env) && !('MY_PAT' in env), removed.join(','));
  check('ci: scrub removes CI job tokens and workflow-command files', removed.length === 12 && !Object.keys(env).some((k) => /^ACTIONS_|TOKEN$|^GITHUB_(?:ENV|OUTPUT|PATH|STATE|STEP_SUMMARY)$/.test(k)), removed.join(','));
  check('ci: scrub keeps ordinary variables', env.PATH === '/usr/bin' && env.HOME === '/home/op' && env.GITHUB_RUN_ID === '1' && env.GITHUB_WORKSPACE === '/w');

  // --- work dir ---
  const wd = defaultWorkdir({ RUNNER_TEMP: join(tmpdir(), 'rt'), GITHUB_RUN_ID: '42', GITHUB_RUN_ATTEMPT: '2' });
  check('ci: default work dir is $RUNNER_TEMP/oac-herdr-<run id>-<attempt>', wd === join(tmpdir(), 'rt', 'oac-herdr-42-2'), wd);
  check('ci: default work dir refuses a missing RUNNER_TEMP', throws(() => defaultWorkdir({ GITHUB_RUN_ID: '1', GITHUB_RUN_ATTEMPT: '1' })));
  check('ci: default work dir refuses a non-numeric run id', throws(() => defaultWorkdir({ RUNNER_TEMP: tmpdir(), GITHUB_RUN_ID: '1/../x', GITHUB_RUN_ATTEMPT: '1' })));
  check('ci: stage refuses a work dir inside the repository', throws(() => ciStage({ workdir: join(REPO_ROOT, 'tmp-ci') })));
  check('ci: cleanup refuses a work dir inside the repository', throws(() => ciCleanup({ workdir: REPO_ROOT })));

  // --- capture-name allowlist ---
  check('ci: capture names the scenarios write are allowlisted', ['pane-smoke.txt', 'pane-2026-09-28-2.1.283-herdr.txt', 'transcript-2026-09-28-2.1.283-herdr.jsonl'].every((n) => CAPTURE_NAME.test(n)));
  check('ci: other names are not', ['run-manifest.json', 'server.log', '../pane-x.txt', 'pane-x.txt/..', 'herdr-config.toml', '.pane-x.txt', 'pane-.txt'].every((n) => !CAPTURE_NAME.test(n)));

  // --- stage gate ---
  stageCase(check, 'clean run stages exactly the manifest and its captures (server.log left behind)', () => {}, true);
  stageCase(check, 'a NOT RUN manifest that is clean still stages', (c) => { c.manifest.outcome = 'NOT RUN'; }, true);
  stageCase(check, 'missing manifest stages nothing', (c) => { c.manifest = undefined; }, false);
  stageCase(check, 'a run whose teardown was not clean stages nothing', (c) => { c.manifest.teardown.clean = false; }, false);
  stageCase(check, 'a manifest with no teardown record stages nothing', (c) => { delete c.manifest.teardown; }, false);
  stageCase(check, 'manifest writtenClean false stages nothing', (c) => { c.manifest.manifestRedaction.writtenClean = false; }, false);
  stageCase(check, 'a capture withheld by the driver stages nothing', (c) => {
    c.manifest.captures[1].written = false;
    c.files['transcript-2026-09-28-2.1.283-herdr.jsonl'] = null;
  }, false);
  stageCase(check, 'a recorded residual hit stages nothing', (c) => { c.manifest.captures[0].redaction.residualGenericHits = [{ label: 'home path', line: 1 }]; }, false);
  stageCase(check, 'a recorded hazard protocol frame stages nothing', (c) => { c.manifest.captures[1].redaction.hazardProtocolFrames = [{ line: 1, method: null }]; }, false);
  stageCase(check, 'a capture missing its redaction report stages nothing', (c) => { delete c.manifest.captures[0].redaction; }, false);
  stageCase(check, 'a listed capture missing on disk stages nothing', (c) => { c.files['pane-smoke.txt'] = null; }, false);
  stageCase(check, 'a capture with a non-allowlisted name stages nothing', (c) => { c.manifest.captures.push({ file: 'server.log', written: true, redaction: { ...CLEAN_REPORT } }); }, false);
  stageCase(check, 'a path-traversal capture name stages nothing', (c) => { c.manifest.captures.push({ file: '../../etc/passwd', written: true, redaction: { ...CLEAN_REPORT } }); }, false);
  stageCase(check, "re-scan: this machine's home path in a capture stages nothing", (c) => { c.files['pane-smoke.txt'] = `cwd ${homedir()}/project\n`; }, false);
  stageCase(check, 're-scan: a generic home path in the manifest stages nothing', (c) => { c.manifest.note = '/home/someoperator/.config'; }, false);
  stageCase(check, 're-scan: a token shape in a transcript stages nothing', (c) => { c.files['transcript-2026-09-28-2.1.283-herdr.jsonl'] = '{"x":"ghp_0123456789abcdefghijABCDEFGHIJ012345"}\n'; }, false);
  stageCase(check, 're-scan: the checkout path in a capture stages nothing', (c) => { c.files['pane-smoke.txt'] = `${REPO_ROOT}\n`; }, false);
  if (process.platform !== 'win32') {
    stageCase(check, 'a capture that is a symlink stages nothing', (c) => {
      c.files['pane-smoke.txt'] = null;
      c.link = true;
      const target = join(c.workdir, 'elsewhere.txt');
      writeFileSync(target, 'OAC-SMOKE-READY\n');
      symlinkSync(target, join(c.out, 'pane-smoke.txt'));
    }, false);
  }
  {
    const ctx = fakeRun();
    try {
      mkdirSync(join(ctx.workdir, 'upload'));
      const res = ciStage({ workdir: ctx.workdir });
      check('ci stage: refuses to stage into an existing upload directory', !res.ok && readdirSync(join(ctx.workdir, 'upload')).length === 0);
    } finally {
      rmSync(ctx.workdir, { recursive: true, force: true });
    }
  }

  {
    const ctx = fakeRun();
    try {
      const before = readFileSync(join(ctx.out, 'pane-smoke.txt'));
      const res = ciStage({ workdir: ctx.workdir });
      writeFileSync(join(ctx.out, 'pane-smoke.txt'), `${homedir()}\n`); // a rewrite after the scan
      check('ci stage: stages the bytes it scanned, not a later re-read', res.ok && readFileSync(join(ctx.workdir, 'upload', 'pane-smoke.txt')).equals(before));
    } finally {
      rmSync(ctx.workdir, { recursive: true, force: true });
    }
  }

  // --- error messages printed to the public log ---
  {
    const msg = redactMessage(`ENOENT: no such file or directory, open '${homedir()}/x/${REPO_ROOT}/y'`);
    check('ci: error messages are redacted before printing', !msg.includes(homedir()) && !msg.includes(REPO_ROOT) && msg.startsWith('ENOENT'), msg);
  }

  // --- runner pre-job hook (runner-hooks/pre-job.sh; the .ps1 twin is not run here) ---
  if (process.platform !== 'win32') {
    const hook = join(REPO_ROOT, 'tools', 'herdr', 'runner-hooks', 'pre-job.sh');
    const OK = {
      GITHUB_REPOSITORY: 'RossGraeber/OAC',
      GITHUB_EVENT_NAME: 'workflow_dispatch',
      GITHUB_WORKFLOW_REF: 'RossGraeber/OAC/.github/workflows/herdr-provider-optin.yml@refs/heads/main',
      GITHUB_REF: 'refs/heads/main',
    };
    const runHook = (over) => {
      const env = { PATH: process.env.PATH, ...OK, ...over };
      for (const [k, v] of Object.entries(env)) if (v === undefined) delete env[k];
      return spawnSync('bash', ['-e', hook], { env, encoding: 'utf8' }).status;
    };
    check('hook: the opt-in workflow on main (dispatch) is allowed', runHook({}) === 0);
    check('hook: the opt-in workflow on main (push) is allowed', runHook({ GITHUB_EVENT_NAME: 'push' }) === 0);
    const refused = {
      'a fork PR through another workflow': { GITHUB_EVENT_NAME: 'pull_request', GITHUB_WORKFLOW_REF: 'RossGraeber/OAC/.github/workflows/boundary-lint.yml@refs/pull/7/merge', GITHUB_REF: 'refs/pull/7/merge' },
      'a fork PR through an edited opt-in workflow': { GITHUB_EVENT_NAME: 'pull_request', GITHUB_WORKFLOW_REF: 'RossGraeber/OAC/.github/workflows/herdr-provider-optin.yml@refs/pull/7/merge', GITHUB_REF: 'refs/pull/7/merge' },
      'pull_request_target on main': { GITHUB_EVENT_NAME: 'pull_request_target' },
      'an issues event on main': { GITHUB_EVENT_NAME: 'issues' },
      'a dispatch from another branch': { GITHUB_WORKFLOW_REF: 'RossGraeber/OAC/.github/workflows/herdr-provider-optin.yml@refs/heads/evil', GITHUB_REF: 'refs/heads/evil' },
      'another workflow on main': { GITHUB_WORKFLOW_REF: 'RossGraeber/OAC/.github/workflows/boundary-lint.yml@refs/heads/main' },
      'another repository': { GITHUB_REPOSITORY: 'someone/OAC', GITHUB_WORKFLOW_REF: 'someone/OAC/.github/workflows/herdr-provider-optin.yml@refs/heads/main' },
      'missing variables': { GITHUB_REPOSITORY: undefined, GITHUB_EVENT_NAME: undefined, GITHUB_WORKFLOW_REF: undefined, GITHUB_REF: undefined },
    };
    for (const [name, over] of Object.entries(refused)) check(`hook: refuses ${name}`, runHook(over) === 1);
  }

  // --- cleanup ---
  {
    const ctx = fakeRun();
    const res = ciCleanup({ workdir: ctx.workdir });
    const git = spawnSync('git', ['status', '--porcelain', '--untracked-files=all', '--ignored=matching'], { cwd: REPO_ROOT, encoding: 'utf8' });
    // In a developer's dirty work tree cleanup correctly reports "not clean"; only the
    // work-dir removal is asserted unconditionally.
    check('ci cleanup: removes the work dir', !existsSync(ctx.workdir));
    check('ci cleanup: result matches the checkout state', res.ok === (git.stdout.trim() === ''), res.problems.join('; '));
  }
}

// Real driver output through the stage gate. `h` carries selftest.mjs's fake-herdr helpers.
export function ciLifecycle(check, h) {
  const b = h.makeBase();
  const workdir = join(b.base, 'ci-workdir');
  mkdirSync(workdir);
  try {
    const run = spawnSync(process.execPath, [h.RUN, '--scenario', 'smoke', '--herdr-bin', h.FAKE, '--out', join(workdir, 'out')], {
      env: h.driverEnv(b, ''),
      encoding: 'utf8',
      timeout: 120000,
    });
    check('ci lifecycle: smoke against the fake herdr ends PASS', run.status === 0, run.stdout + run.stderr);
    const res = ciStage({ workdir });
    const uploaded = existsSync(join(workdir, 'upload')) ? readdirSync(join(workdir, 'upload')).sort() : [];
    check('ci lifecycle: the stage gate accepts real driver output', res.ok, res.problems.join('; '));
    check('ci lifecycle: exactly the run manifest and the pane capture are staged', JSON.stringify(uploaded) === JSON.stringify(['pane-smoke.txt', 'run-manifest.json']), uploaded.join(','));
    check('ci lifecycle: nothing from the scratch directory is staged', !uploaded.some((n) => /scratch|server\.log|herdr-config|env-probe/.test(n)));
    const clean = ciCleanup({ workdir });
    check('ci lifecycle: cleanup removes the work dir', !existsSync(workdir), clean.problems.join('; '));
  } finally {
    rmSync(b.base, { recursive: true, force: true });
  }
}
