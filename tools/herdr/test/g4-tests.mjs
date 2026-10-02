// K8 (#131) G4 tests, run by the driver self-test (`node tools/herdr/run.mjs --self-test`).
//
// Unit half: the pinned G4 criteria (and their drift refusal), the per-invocation Codex launch
// rules, the transcript facts against the REAL committed human-run fixture, the extension-id
// sanitizer, and a REPLAY of the committed fixture's client requests against the reconstructed
// server (tools/herdr/gate-servers/g4-server.mjs): every response and push the reconstruction
// gives must match the fixture's, volatile fields (pids, session ids, message ids) aside. The
// replay shows the reconstruction reproduces the recorded wire shapes; it cannot show that it
// behaves like the uncommitted original in any situation the fixture did not record.
//
// Lifecycle half: scenarios/g4-mcp-dual-era.mjs end to end through run.mjs against the fake
// herdr, the fake Claude Code and the fake Codex (TEST DOUBLES). One case is traced
// (test/fs-trace.mjs) to show, from the trace, that no process wrote under either harness home,
// created a Codex config file, or copied anything out of the Codex home. These prove the
// scenario's and the driver's behavior only; a live G4 run through herdr is UNVERIFIED.

import { spawn, spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, realpathSync, rmSync, utimesSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { basename, dirname, join, relative, resolve, isAbsolute } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createInterface } from 'node:readline';

import {
  BASELINE_TRANSCRIPT, FIXTURE_DIR, G4_LAUNCH, OAC_EXT, OAC_EXT_PLACEHOLDER, G4_CRITERIA_SHA256, G4_REFERENCE, DEFAULT_PROMPTS,
  readG4Criteria, validateCodexLaunch, codexLaunchParamProblem, validatePaneEnv, defaultCodexLaunch, assertNotInjected, fixtureNames, unverifiedNames, parseG4Transcript, g4Facts,
  modernRequests, roles, sanitizeG4Transcript, sanitizeG4Text, placeholderIntegrity, HUMAN_RUN_PORTS, DEFAULT_PORTS, codexSessions,
} from '../lib/g4.mjs';
import { CriteriaDriftError, parseCriteriaSection } from '../lib/gate-common.mjs';
import { SCORES, ReportError, evaluateG4, parseG4OperatorScores, wireEvidence, writeRefusal, fixtureWithheld, renderReport, draftManifestEntries, versionMatchesLastTested } from '../lib/g4-report.mjs';
import { createRedactor } from '../lib/redact.mjs';
import { paneArgv, splitCommandLine } from '../lib/g2.mjs';
import { processTable, spawnLongRunning } from '../lib/proc.mjs';
import { parseClaudeVersions, parseCodexVersions } from '../lib/pins.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..', '..');
const read = (p) => readFileSync(p, 'utf8');
const BASELINE = read(join(REPO, BASELINE_TRANSCRIPT));
const PINS = read(join(REPO, 'docs', 'planning', 'PINS.md'));
// PINS.md's last tested versions: the fakes report them unless a case sets others (#216).
const CPIN = parseClaudeVersions(PINS).lastTested;
const XPIN = parseCodexVersions(PINS).lastTested;
const REPORT = join(REPO, 'tools', 'herdr', 'lib', 'g4-report.mjs');
const SERVER = join(REPO, 'tools', 'herdr', 'gate-servers', 'g4-server.mjs');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const throws = (fn, cls, re) => {
  try {
    fn();
    return false;
  } catch (e) {
    return (!cls || e instanceof cls) && (!re || re.test(e.message));
  }
};
const freePort = () =>
  new Promise((res) => {
    const s = createServer();
    s.listen(0, '127.0.0.1', () => {
      const { port } = s.address();
      s.close(() => res(port));
    });
  });

// Drift of a pinned criteria reference, in a throwaway git repository (K7's review pattern).
export function criteriaDriftChecks(check, { label, refPath, readCriteria, pin }) {
  const refText = read(join(REPO, refPath)).replace(/\r\n/g, '\n'); // LF as committed, also on a CRLF checkout
  const lines = refText.split('\n');
  const idx = lines.map((l, i) => (/^- \[[ x]\] /.test(l) ? i : -1)).filter((i) => i !== -1);
  const gr = mkdtempSync(join(tmpdir(), 'oac-k8-crit-'));
  try {
    const git = (...a) => spawnSync('git', ['-c', 'user.email=t@example.invalid', '-c', 'user.name=t', '-c', 'commit.gpgsign=false', ...a], { cwd: gr, encoding: 'utf8', timeout: 15000 });
    const put = (text) => {
      mkdirSync(join(gr, dirname(refPath)), { recursive: true });
      writeFileSync(join(gr, refPath), text);
    };
    git('init', '-q');
    put(refText);
    git('add', '.');
    git('commit', '-q', '-m', 'ref');
    const same = readCriteria(gr);
    const reworded = lines.map((l, i) => (i === idx[0] ? `${l} (reworded)` : l)).join('\n');
    put(reworded);
    const uncommitted = readCriteria(gr);
    const drift = (text) => {
      put(text);
      git('commit', '-q', '-am', 'drift');
      return throws(() => readCriteria(gr), CriteriaDriftError, /the reference changed/);
    };
    check(`${label} criteria drift: the committed reference as K8 found it passes`, same.reference.criteriaSha256 === pin && same.reference.workingTreeMatchesHead);
    check(`${label} criteria drift: an UNCOMMITTED reword is never read (HEAD is), and is reported`, JSON.stringify(uncommitted.criteria) === JSON.stringify(same.criteria) && uncommitted.reference.workingTreeMatchesHead === false);
    check(`${label} criteria drift: a committed reword is refused`, drift(reworded));
    const swap = [...lines];
    [swap[idx[0]], swap[idx[1]]] = [swap[idx[1]], swap[idx[0]]];
    git('checkout', '-q', 'HEAD~1', '--', refPath);
    git('commit', '-q', '-am', 'restore');
    check(`${label} criteria drift: a committed reorder is refused`, readCriteria(gr).reference.criteriaSha256 === pin && drift(swap.join('\n')));
  } finally {
    rmSync(gr, { recursive: true, force: true });
  }
}

// --- replay of the committed fixture against the reconstructed server ---------------------

function startStdio(file, env, cwd) {
  const child = spawn(process.execPath, [file], { cwd, env: { ...process.env, ...env }, stdio: ['pipe', 'pipe', 'ignore'] });
  const waiting = new Map();
  createInterface({ input: child.stdout }).on('line', (line) => {
    let m;
    try {
      m = JSON.parse(line);
    } catch {
      return;
    }
    if (m.id !== undefined && waiting.has(JSON.stringify(m.id))) {
      waiting.get(JSON.stringify(m.id))(m);
      waiting.delete(JSON.stringify(m.id));
    }
  });
  return {
    child,
    send: (payload) =>
      new Promise((res) => {
        if (payload.id !== undefined) waiting.set(JSON.stringify(payload.id), res);
        child.stdin.write(`${JSON.stringify(payload)}\n`);
        if (payload.id === undefined) setTimeout(res, 30);
        else setTimeout(() => res(null), 3000);
      }),
  };
}

const VOLATILE = /("served_by_pid":)\d+|("message_id":)"[^"]*"|pid \d+/g;
const norm = (x) => JSON.stringify(x ?? null).replace(VOLATILE, (m, a, b) => (a ? `${a}0` : b ? `${b}"-"` : 'pid <PID>'));

export async function replayG4Fixture() {
  const dir = mkdtempSync(join(tmpdir(), 'oac-g4-replay-'));
  const copy = join(dir, 'g4-server.mjs');
  copyFileSync(SERVER, copy);
  const [p1, p2] = [await freePort(), await freePort()];
  const legacy = startStdio(copy, { G4_HTTP_PORT: String(p1) }, dir);
  const modern = startStdio(copy, { G4_STDIO_MODERN: '1', G4_HTTP_PORT: String(p2) }, dir);
  const tpath = join(dir, 'transcript.jsonl');
  const facts = () => g4Facts(existsSync(tpath) ? parseG4Transcript(read(tpath), { completeLinesOnly: true }) : []);
  try {
    for (let i = 0; i < 100 && facts().listening.length < 2; i++) await sleep(50);
    const base = parseG4Transcript(BASELINE);
    const bf = g4Facts(base);
    const r = roles(bf);
    const sid = new Map();
    const pushesNow = () => facts().pushes.length;
    const wake = async (want) => {
      const before = pushesNow();
      const now = new Date(Date.now() + 1000);
      const t = join(dir, 'wake.trigger');
      if (existsSync(t)) utimesSync(t, now, now);
      else writeFileSync(t, '');
      for (let i = 0; i < 60 && pushesNow() < before + want; i++) await sleep(50);
    };
    const wakeAfter = [bf.stdioToolsList.find((x) => x.pid === r.legacyPid)?.resLine, bf.toolCalls.filter((c) => c.era === 'modern').at(-1)?.resLine];
    for (const e of base) {
      if (e.direction === 'client->server' && (e.pid === r.legacyPid || e.pid === r.modernPid)) {
        if (e.surface === 'stdio') await (e.pid === r.legacyPid ? legacy : modern).send(e.payload);
        else {
          const h = { ...e.payload.headers };
          if (h['mcp-session-id']) h['mcp-session-id'] = sid.get(h['mcp-session-id']) ?? h['mcp-session-id'];
          delete h['accept-encoding'];
          const res = await fetch(`http://127.0.0.1:${p1}${e.payload.url}`, { method: e.payload.method, headers: h, body: e.payload.method === 'POST' ? JSON.stringify(e.payload.body) : undefined });
          await res.text();
          if (e.payload.body?.method === 'initialize') {
            const bres = base.find((x) => x.line > e.line && x.direction === 'server->client' && x.surface === 'http' && x.headers?.['Mcp-Session-Id']);
            sid.set(bres.headers['Mcp-Session-Id'], res.headers.get('mcp-session-id'));
          }
        }
      }
      if (wakeAfter.includes(e.line)) await wake(2);
    }
    await sleep(200);
    const nf = facts();
    const nr = roles(nf);
    const shape = (f, rl) => {
      const roleOf = (pid) => (pid === rl.legacyPid ? 'legacy' : pid === rl.modernPid ? 'modern' : null);
      return {
        requests: f.requests.filter((q) => roleOf(q.pid)).map((q) => `${roleOf(q.pid)} ${q.surface} ${q.era} ${q.method} ${q.response ? `${q.response.status ?? ''} ${norm(q.response.error ?? q.response.result)} ${q.response.headers ? Object.keys(q.response.headers).join() : ''}` : 'no response'}`),
        // Per role, in order: the two copies push on the same wake and may interleave either way.
        pushes: ['legacy', 'modern'].flatMap((role) => f.pushes.filter((p) => roleOf(p.pid) === role).map((p) => `${role} ${p.era} ${norm(p.content)} ${norm(p.meta)}`)),
        gets: f.getOrDelete.filter((g) => roleOf(g.pid)).map((g) => `${g.method} ${g.note}`),
        notes: f.instances.map((i) => `${roleOf(i.pid)} ${i.stdioModern}`).filter((x) => !x.startsWith('null')),
      };
    };
    return { base: shape(bf, r), run: shape(nf, nr), nf };
  } finally {
    // Wait for both to exit before removing their working directory: on Windows a process
    // that is still exiting holds it open and rmSync fails with EPERM.
    await Promise.all([legacy.child, modern.child].map((c) => killAndWait(c)));
    rmSync(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
  }
}

// SIGKILL a child and wait (bounded) for it to exit.
export function killAndWait(child, timeoutMs = 5000) {
  if (child.exitCode !== null || child.signalCode !== null) return Promise.resolve();
  return new Promise((res) => {
    const t = setTimeout(res, timeoutMs);
    child.once('exit', () => {
      clearTimeout(t);
      res();
    });
    child.kill('SIGKILL');
  });
}

// #243 (Windows only): a REAL child process (node, running a script named codex.js) started
// through the driver's own spawn path (lib/proc.mjs spawnLongRunning: Node's Windows argument
// quoting) with the default G4 launch's overrides, and a second with hard-to-quote arguments.
// Each one's Win32_Process.CommandLine, read back through processTable(), must split to
// exactly the argv it was given, and the default launch must give matchesExpected === true.
async function windowsLaunchRoundTrip(check) {
  if (process.platform !== 'win32') {
    console.log('  skip  g4 codex launch #243: the real-process argv round trip runs on Windows only');
    return;
  }
  const dir = mkdtempSync(join(tmpdir(), 'oac-g4-argv-'));
  const kids = [];
  try {
    const script = join(dir, 'codex.js');
    writeFileSync(script, 'setTimeout(() => {}, 60000);\n');
    const overrides = defaultCodexLaunch(17448).slice(1);
    const hard = ['features.x=a "b" c', 'C:\\dir with space\\', 'a\\\\"b', '', 'tab\there', '\\\\server\\share\\x', 'end\\'];
    for (const args of [overrides, hard]) kids.push({ args, ...spawnLongRunning(process.execPath, [script, ...args], { env: process.env, cwd: dir }) });
    const table = processTable();
    const split = kids.map((k) => {
      const r = table?.get(k.child.pid);
      return typeof r?.commandLine === 'string' ? splitCommandLine(r.commandLine) : null;
    });
    const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
    check('g4 codex launch #243 (Windows): a real child\'s Win32 command line, read through processTable(), splits back to exactly the argv it was spawned with (the default G4 launch, and hard-to-quote arguments)', kids.every((k, i) => same(split[i], [process.execPath, script, ...k.args])), JSON.stringify(split));
    const pa = paneArgv([kids[0].child.pid], table, { allow: overrides, expectArgsAfterCodex: overrides });
    check('g4 codex launch #243 (Windows): the default G4 launch\'s real process gives matchesExpected === true, its overrides kept verbatim', pa.proof.found && pa.proof.codexToken === 'codex.js' && pa.proof.matchesExpected === true && same(pa.proof.argsAfterCodex, overrides), JSON.stringify(pa.proof));
  } finally {
    await Promise.all(kids.map((k) => killAndWait(k.child)));
    rmSync(dir, { recursive: true, force: true });
  }
}

// #249 (PR #248 review 2): the refusal-before-run path is wired end to end, on every platform
// (the lifecycle cases that also cover it are POSIX only). run.mjs is started for real with a
// refused codexLaunch, a herdr that does not exist and a private os.tmpdir() (TMPDIR, TEMP and
// TMP all pointed at one fresh directory). Removing the validateParams hook from the G4
// scenario, or the call to it from run.mjs, lets the run go on to create its scratch directory
// and fail on the missing herdr: this check then fails.
async function validateParamsWired(check, SECRET) {
  const sc = (await import('../scenarios/g4-mcp-dual-era.mjs')).default;
  const bad = ['codex', '-c', `mcp_servers.g4http.url="http://op:${SECRET}@127.0.0.1:37548/mcp"`];
  const direct = typeof sc.validateParams === 'function' ? sc.validateParams({ params: { codexLaunch: JSON.stringify(bad) }, launch: sc.defaults?.launch ?? [] }) : null;
  check('g4 validateParams #249: the G4 scenario exports a validateParams that refuses a bad codexLaunch without quoting it', /^codexLaunch refused: /.test(direct ?? '') && !direct.includes(SECRET), String(direct));
  const priv = mkdtempSync(join(tmpdir(), 'oac-herdr-g4vp-'));
  try {
    const tmp = join(priv, 'tmp');
    mkdirSync(tmp);
    const out = join(priv, 'out');
    const res = spawnSync(process.execPath, [join(REPO, 'tools', 'herdr', 'run.mjs'), '--scenario', 'g4-mcp-dual-era', '--herdr-bin', join(priv, 'no-such-herdr.mjs'), '--out', out, '--param', `codexLaunch=${JSON.stringify(bad)}`], {
      env: { ...process.env, TMPDIR: tmp, TEMP: tmp, TMP: tmp },
      encoding: 'utf8',
      timeout: 60000,
      windowsHide: true,
    });
    const scratch = readdirSync(tmp).filter((n) => n.startsWith('oac-herdr-scratch-'));
    check('g4 validateParams #249: run.mjs refuses a bad codexLaunch before anything is created (exit 2; no output dir, no manifest, no oac-herdr-scratch-* dir; the value on no output)', res.status === 2 && /g4-mcp-dual-era: codexLaunch refused: .*refused before anything was created, nothing recorded/.test(res.stderr) && !existsSync(out) && scratch.length === 0 && !`${res.stdout}${res.stderr}`.includes(SECRET), JSON.stringify({ status: res.status, stderr: res.stderr.slice(0, 400), scratch, out: existsSync(out) }));
  } finally {
    rmSync(priv, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
  }
}

export async function g4Unit(check) {
  // --- criteria --------------------------------------------------------------------------------
  const { criteria: crit, reference } = readG4Criteria(REPO);
  check('g4: the five G4 criteria are read verbatim from the committed oac-gates reference and match the K8 pin', crit.length === 5 && /^The legacy path registers as a channel/.test(crit[0]) && /rejected as a channel/.test(crit[4]) && reference.criteriaSha256 === G4_CRITERIA_SHA256 && reference.path === G4_REFERENCE);
  check('g4: a reference with the wrong number of criteria is refused', throws(() => parseCriteriaSection('## Pass criteria\n\n- [ ] one\n\n## Next\n', { count: 5, path: 'x' })));
  criteriaDriftChecks(check, { label: 'g4', refPath: G4_REFERENCE, readCriteria: readG4Criteria, pin: G4_CRITERIA_SHA256 });
  check('g4 criteria drift: evaluateG4 itself refuses reordered or reworded criteria', throws(() => evaluateG4({ manifest: { outcome: 'NOT RUN' }, baselineText: BASELINE, criteria: [crit[1], crit[0], ...crit.slice(2)] }), CriteriaDriftError) && throws(() => evaluateG4({ manifest: { outcome: 'NOT RUN' }, baselineText: BASELINE, criteria: [...crit.slice(0, 4), `${crit[4]} (edited)`] }), CriteriaDriftError));

  // --- the Codex launch: per invocation only ------------------------------------------------------
  const ok = validateCodexLaunch(defaultCodexLaunch(17448));
  check('g4 codex launch: the default is a per-invocation MCP-server url override, nothing else', ok.ok && ok.overrides.length === 1 && ok.overrides[0].key === 'mcp_servers.g4http.url' && JSON.stringify(defaultCodexLaunch(17448)) === '["codex","-c","mcp_servers.g4http.url=\\"http://127.0.0.1:17448/mcp\\""]');
  await windowsLaunchRoundTrip(check);
  check('g4 codex launch: a feature flag beside the url is allowed (the row-41 opt-in, as a method change)', validateCodexLaunch(['codex', '-c', 'mcp_servers.g4.url="http://127.0.0.1:17458/mcp"', '--config', 'features.mcp_2026_07_28=true']).ok);
  // #244 (PR #242 review note A): override VALUES are allowlisted too; a refusal records nothing.
  const url = (u) => ['codex', '-c', `mcp_servers.g4.url=${u}`];
  const goodValues = [
    url('"http://127.0.0.1:17458/mcp"'), url('"http://[::1]:17458/mcp"'), url('"http://localhost:17458/mcp"'), url('"https://127.0.0.1/mcp"'),
    url('"http://LOCALHOST:1/"'), url('"http://127.0.0.1:65535"'), url('"http://127.0.0.1:17458/a/b-c_d.e~f"'),
    [...url('"http://127.0.0.1:17458/mcp"'), '-c', 'mcp_servers.g4.enabled=true', '-c', 'mcp_servers.g4.startup_timeout_sec=30', '-c', 'mcp_servers.g4.tool_timeout_sec=12.5', '-c', 'features.x=false'],
    // #249: zero and a zero-led fraction are TOML numbers.
    [...url('"http://127.0.0.1:17458/mcp"'), '-c', 'mcp_servers.g4.startup_timeout_sec=0', '-c', 'mcp_servers.g4.tool_timeout_sec=0.5'],
  ];
  check('g4 codex launch #244: loopback URLs (127.0.0.1, [::1], localhost; http or https; any port and path) and boolean/number scalars are accepted', goodValues.every((a) => validateCodexLaunch(a).ok), JSON.stringify(goodValues.filter((a) => !validateCodexLaunch(a).ok)));
  const SECRET = 'S3CRETvalue42';
  const badValues = [
    ['userinfo', url(`"http://user:${SECRET}@127.0.0.1:17458/mcp"`)],
    ['userinfo, user only', url(`"http://${SECRET}@localhost/mcp"`)],
    ['non-loopback host', url(`"http://${SECRET}.example.net/mcp"`)],
    ['non-loopback IP', url('"http://10.0.0.5:17458/mcp"')],
    ['loopback lookalike host', url('"http://127.0.0.1.example.net/mcp"')],
    ['localhost subdomain', url('"http://localhost.example.net/mcp"')],
    ['userinfo hiding a remote host', url(`"http://127.0.0.1@${SECRET}.example.net/mcp"`)],
    ['query string', url(`"http://127.0.0.1:17458/mcp?key=${SECRET}"`)],
    ['fragment', url(`"http://127.0.0.1:17458/mcp#${SECRET}"`)],
    ['other scheme', url('"ws://127.0.0.1:17458/mcp"')],
    ['unquoted url', url('http://127.0.0.1:17458/mcp')],
    ['port out of range', url('"http://127.0.0.1:70000/mcp"')],
    ['port zero', url('"http://127.0.0.1:0/mcp"')],
    ['escape in the string', url('"http://127.0.0.1:17458/mcp\\u0040x"')],
    ['whitespace', url(`"http://127.0.0.1:17458/mcp ${SECRET}"`)],
    ['string for a boolean', [...url('"http://127.0.0.1:17458/mcp"'), '-c', `mcp_servers.g4.enabled="${SECRET}"`]],
    ['string for a feature flag', [...url('"http://127.0.0.1:17458/mcp"'), '-c', `features.x=${SECRET}`]],
    ['string for a timeout', [...url('"http://127.0.0.1:17458/mcp"'), '-c', `mcp_servers.g4.tool_timeout_sec="${SECRET}"`]],
    ['negative timeout', [...url('"http://127.0.0.1:17458/mcp"'), '-c', 'mcp_servers.g4.tool_timeout_sec=-1']],
    // #249: the path is [A-Za-z0-9._~/-] only; an encoded query or a key=value-shaped segment is refused.
    ['percent-encoded query in the path', url(`"http://127.0.0.1:17458/mcp%3Ftoken=${SECRET}"`)],
    ['token-shaped path segment', url(`"http://127.0.0.1:17458/mcp;token=${SECRET}"`)],
    ['at sign in the path', url(`"http://127.0.0.1:17458/mcp/@${SECRET}"`)],
    ['sub-delimiters in the path', url(`"http://127.0.0.1:17458/mcp/$${SECRET}!*'(),+:"`)],
    // #249: TOML rejects a leading zero.
    ['leading-zero timeout', [...url('"http://127.0.0.1:17458/mcp"'), '-c', 'mcp_servers.g4.tool_timeout_sec=007']],
    ['leading-zero fractional timeout', [...url('"http://127.0.0.1:17458/mcp"'), '-c', 'mcp_servers.g4.startup_timeout_sec=00.5']],
  ];
  const refusals = badValues.map(([what, a]) => [what, validateCodexLaunch(a)]);
  check('g4 codex launch #244: a userinfo URL, a non-loopback URL, a query, a fragment, another scheme and a non-boolean/non-number scalar are all refused', refusals.every(([, v]) => !v.ok), JSON.stringify(refusals.filter(([, v]) => v.ok).map(([w]) => w)));
  check('g4 codex launch #244: a refusal records nothing of the launch (no overrides, the reason names a position, never the text)', refusals.every(([, v]) => v.overrides.length === 0 && !v.why.includes(SECRET) && !/example\.net|10\.0\.0\.5|ws:|70000/.test(v.why) && /argument \d+/.test(v.why)), JSON.stringify(refusals.map(([w, v]) => [w, v.why])));
  check('g4 codex launch #244: an unknown key is refused without echoing it either', ((v) => !v.ok && v.overrides.length === 0 && !v.why.includes(SECRET))(validateCodexLaunch(['codex', '-c', `${SECRET}=1`, ...url('"http://127.0.0.1:1/mcp"').slice(1)])));
  // #249 (PR #248 review 1): the two refusals before a key is even parsed -- an argument that is
  // not `-c`/`--config`, and a `-c` with no key=value after it -- name the rule and position
  // only. A mutation that echoed the argument would put SECRET in `why`.
  const early = [
    ['a bare argument in place of -c', ['codex', SECRET, ...url('"http://127.0.0.1:1/mcp"').slice(1)], /argument 1 is not a per-invocation/],
    ['a flag in place of -c', ['codex', `--${SECRET}`, ...url('"http://127.0.0.1:1/mcp"').slice(1)], /argument 1 is not a per-invocation/],
    ['a flag=value in place of -c', ['codex', ...url('"http://127.0.0.1:1/mcp"').slice(1), `--token=${SECRET}`], /argument 3 is not a per-invocation/],
    ['-c followed by no key=value', ['codex', '-c', SECRET, ...url('"http://127.0.0.1:1/mcp"').slice(1)], /at argument 1 needs a key=value/],
    ['--config followed by an empty key', ['codex', '--config', `=${SECRET}`], /at argument 1 needs a key=value/],
    ['-c as the last argument', ['codex', ...url('"http://127.0.0.1:1/mcp"').slice(1), '-c'], /at argument 3 needs a key=value/],
  ];
  const earlyRefusals = early.map(([what, a, re]) => [what, validateCodexLaunch(a), re]);
  check('g4 codex launch #249: "not a -c override" and bare `-c` refusals name the rule and position, never the argument text', earlyRefusals.every(([, v, re]) => !v.ok && v.overrides.length === 0 && re.test(v.why) && !v.why.includes(SECRET)), JSON.stringify(earlyRefusals.map(([w, v]) => [w, v.why])));
  check('g4 codexLaunch param #249: the same refusals through the param never quote the value', early.every(([, a]) => ((p) => /^codexLaunch refused: /.test(p ?? '') && !p.includes(SECRET))(codexLaunchParamProblem({ codexLaunch: JSON.stringify(a) }))));
  await validateParamsWired(check, SECRET);
  check('g4 codexLaunch param #244: refused before the run, the reason never quoting the value; no param or a valid one passes', codexLaunchParamProblem({}) === null && codexLaunchParamProblem({ codexLaunch: JSON.stringify(defaultCodexLaunch(17458)) }) === null && /^codexLaunch refused: /.test(codexLaunchParamProblem({ codexLaunch: JSON.stringify(badValues[0][1]) }) ?? '') && !codexLaunchParamProblem({ codexLaunch: JSON.stringify(badValues[0][1]) }).includes(SECRET) && codexLaunchParamProblem({ codexLaunch: `not json ${SECRET}` }) === 'codexLaunch must be a JSON argv');
  const bad = [
    [['codex', '--profile', 'p', '-c', 'mcp_servers.g4.url="u"'], /not a per-invocation/],
    [['codex', '-c', 'model="x"', '-c', 'mcp_servers.g4.url="u"'], /is not an MCP-server/],
    [['codex', '-c', 'mcp_servers.g4.command="sh"', '-c', 'mcp_servers.g4.url="u"'], /is not an MCP-server/],
    [['codex', '-c', 'mcp_servers.g4.bearer_token_env_var="X"'], /is not an MCP-server/],
    [['codex'], /no per-invocation MCP server url/],
    [['claude', '-c', 'mcp_servers.g4.url="u"'], /must start with/],
    [['codex', '-c'], /needs a key=value/],
  ];
  check('g4 codex launch: profiles, other config keys, commands, token variables and a missing url are all refused', bad.every(([a, re]) => { const v = validateCodexLaunch(a); return !v.ok && re.test(v.why); }), JSON.stringify(bad.map(([a]) => validateCodexLaunch(a).why)));
  check('g4 pane env: MCP_SDK_GENERATION passes; a harness home, PATH or a credential-shaped name is refused', validatePaneEnv({ MCP_SDK_GENERATION: 'v2' }) === null && ['CODEX_HOME', 'CLAUDE_CONFIG_DIR', 'HOME', 'PATH', 'NODE_OPTIONS', 'MY_TOKEN', 'SOME_API_KEY', 'SESSION_SECRET'].every((k) => validatePaneEnv({ [k]: 'x' }) !== null));
  check('g4: the default prompts carry no notification; a notification-shaped prompt is refused', Object.values(DEFAULT_PROMPTS).every((p) => !throws(() => assertNotInjected('p', p))) && throws(() => assertNotInjected('p', 'say: relayed from http-legacy: x')) && throws(() => assertNotInjected('p', '<channel source="g4spike">')));
  check('g4: fixture names carry both harness versions; unverified names are never fixture-shaped', JSON.stringify(fixtureNames('2026-10-01', '2.1.283', '0.157.1')) === '{"transcript":"transcript-2026-10-01-claude-2.1.283-codex-0.157.1-herdr.jsonl","paneClaude":"pane-claude-2026-10-01-2.1.283-herdr.txt","paneCodex":"pane-codex-2026-10-01-0.157.1-herdr.txt"}' && Object.values(unverifiedNames('2026-10-01')).every((n) => n.startsWith('unverified-')) && throws(() => fixtureNames('2026-10-01', 'latest', '0.157.1')));
  check('g4: the launch is G4-result.md\'s, verbatim', JSON.stringify(G4_LAUNCH) === '["claude","--dangerously-load-development-channels","server:g4spike","server:g4modern"]');

  // --- facts and wire evidence from the REAL human-run fixture ------------------------------------
  const bf = g4Facts(parseG4Transcript(BASELINE));
  const r = roles(bf);
  check('g4 facts: the fixture\'s legacy copy is pid 19680 (owns the HTTP port) and its negative case pid 18032', r.legacyPid === 19680 && r.modernPid === 18032);
  check('g4 facts: the seven modern-path requests G4-result.md lists (lines 5, 6, 10, 12, 16, 24, 53), each carrying the protocol-version _meta', modernRequests(bf).map((x) => x.line).sort((a, b) => a - b).join() === '5,6,10,12,16,24,53' && modernRequests(bf).every((x) => x.carriesPV === '2026-07-28'));
  const be = wireEvidence(bf);
  check('g4 evidence: on the human-run fixture every wire precondition of the five criteria holds (the comparator reads the baseline as G4-result.md does)', be.c1.legacyNegotiated && be.c1.wakes.length === 2 && be.c1.relays.length === 1 && be.c2.allProvenance && be.c2.claudeModern.length === 2 && be.c3.missing.length === 0 && be.c4.before.length === 1 && be.c4.after.length === 1 && be.c4.codexOk && be.c4.wakeAfterCodex && be.c4.errorsOnLegacy.length === 0 && be.c5.modernOnly && be.c5.modernPushes.length === 2 && be.c2.codexEras.join() === '2025-11-25', JSON.stringify({ c2: be.c2.codexEras, c4: be.c4.errorsOnLegacy }));

  const noCodex = BASELINE.split('\n').filter((l) => !/codex-mcp-client|"pid":17580/.test(l)).join('\n');
  const nc = wireEvidence(g4Facts(parseG4Transcript(noCodex)));
  check('g4 evidence: with the Codex traffic removed, criterion 4\'s before/after-Codex evidence is empty (never vacuously met)', nc.c4.before.length === 0 && nc.c4.after.length === 0 && !nc.c4.wakeAfterCodex && nc.c4.codexInits.length === 0);

  // --- the extension-identifier sanitizer (a real hazard of run.mjs's identity redaction) ---------
  const r2 = createRedactor({ home: '/home/rossg', username: 'rossg', hostname: 'box-1' });
  const raw = r2.redactJsonl(BASELINE).text;
  const san = sanitizeG4Transcript(BASELINE);
  const sanRed = r2.redactJsonl(san.text).text;
  check('g4 sanitize: on a machine whose username is a substring of the identifier, redaction alone rewrites the provenance key (why the sanitizer exists)', !raw.includes(OAC_EXT) && raw.includes('io.github.<USER>'));
  check('g4 sanitize: the sanitizer replaces every occurrence with the placeholder first, so redaction leaves the key intact and the facts still find the provenance', san.report.extensionIdReplaced === (BASELINE.split(OAC_EXT).length - 1) && sanRed.includes(OAC_EXT_PLACEHOLDER) && wireEvidence(g4Facts(parseG4Transcript(sanRed))).c2.allProvenance);

  // Review finding (K8): the pane captures need the same substitution, and the placeholder must
  // survive redaction too. The prompts ask for the full result, so a pane can show the identifier.
  const pane = `### section\n  ⎿  {"_meta":{"${OAC_EXT}":{"surface":"http-modern"}}}\nprovenance under ${OAC_EXT}\n`;
  const users = ['rossg', 'graeber', 'ross', 'github', 'sion', 'tens', 'oac', 'ext', 'extid'];
  const rawCorrupt = users.filter((u) => { const t = createRedactor({ username: u, home: `/home/${u}`, hostname: 'box-1' }).redactText(pane).text; return !t.includes(OAC_EXT); });
  check('g4 sanitize (panes): without the sanitizer, redaction silently corrupts the identifier in pane text for several usernames', ['rossg', 'graeber', 'ross', 'github', 'sion'].every((u) => rawCorrupt.includes(u)), rawCorrupt.join());
  const sp = sanitizeG4Text(pane);
  const survives = users.map((u) => [u, placeholderIntegrity(createRedactor({ username: u, home: `/home/${u}`, hostname: 'box-1' }).redactText(sp.text).text, sp.replaced)]);
  check('g4 sanitize (panes): with it, the placeholder survives redaction intact for every one of those usernames, including ones matching inside the old placeholder (sion, tens, ext)', sp.replaced === 2 && survives.every(([, v]) => v.ok), JSON.stringify(survives.filter(([, v]) => !v.ok)));
  const adversarial = placeholderIntegrity(createRedactor({ username: 'oac_ext', home: '/home/oac_ext', hostname: 'box-1' }).redactText(sp.text).text, sp.replaced);
  check('g4 sanitize: a username that does match inside the placeholder is caught by the integrity count (fail closed), and a wrapped identifier fragment is caught too', !adversarial.ok && !placeholderIntegrity(`${OAC_EXT_PLACEHOLDER} io.github.ross\ngraeber/oac-session-channels`, 1).ok);

  // #148: a wrapped identifier escapes the sanitizer, and a username matching the unbroken side
  // of the wrap is then redacted, so neither literal anchor survives in the raw text.
  const redAs = (u, t) => createRedactor({ username: u, home: `/home/${u}`, hostname: 'box-1' }).redactText(t).text;
  const exact = [['github', OAC_EXT.replace('oac-sess', 'oac-sess\n')], ['session', OAC_EXT.replace('io.git', 'io.git\n')]].map(([u, w]) => {
    const t = redAs(u, `${OAC_EXT_PLACEHOLDER}\nprovenance under ${w}\n`);
    return { u, t, anchorsGone: !/oac-session-channels|io\.github\./i.test(t), pi: placeholderIntegrity(t, 1) };
  });
  check('g4 sanitize (#148): a wrapped identifier whose unbroken side matched the username (github / break in oac-session-channels; session / break in io.github.) is flagged as a fragment though no literal anchor survives and the count still lines up', exact.every((x) => x.anchorsGone && x.t.includes('<USER>') && x.pi.found === 1 && x.pi.fragment && !x.pi.ok), JSON.stringify(exact));
  const wrapAt = (t, w, nl) => t.match(new RegExp(`.{1,${w}}`, 'gs')).join(nl);
  const wrapUsers = ['rossg', 'graeber', 'ross', 'github', 'sion', 'session', 'channels', 'tens', 'oac', 'rossgraeber'];
  const wrapMiss = [];
  let wrapCases = 0;
  // Wrap decorations a TUI or terminal may put at a soft wrap: a box border, a quote / output /
  // bullet prefix, an SGR reset, NEL, U+2028, a zero-width space, a soft hyphen.
  const wrapSeps = ['\n', '\r\n', '│\n│ ', '\n▌ ', '\n█', '\n| ', '\n⎿ ', '\n• ', '\n› ', '\x1b[0m\n\x1b[2m', '\u0085', ' ', '​', '­\n'];
  for (const nl of wrapSeps) for (let w = 4; w <= 60; w++) for (const u of wrapUsers) {
    const raw = wrapAt(`  ⎿  provenance under ${OAC_EXT} ok`, w, nl);
    const sp2 = sanitizeG4Text(raw);
    const v = placeholderIntegrity(redAs(u, sp2.text), sp2.replaced);
    wrapCases++;
    if (v.ok !== (sp2.replaced === 1)) wrapMiss.push({ nl, w, u, replaced: sp2.replaced, v });
  }
  check('g4 sanitize (#148): at every pane width 4-60, with LF, CRLF, box-border, quote/output/bullet-prefix, ANSI, NEL, U+2028, zero-width-space and soft-hyphen wraps, for every username matching a piece of the identifier, a split identifier fails integrity and an unsplit one passes', wrapMiss.length === 0 && wrapCases === wrapSeps.length * 57 * wrapUsers.length, JSON.stringify(wrapMiss.slice(0, 3)));
  // Only the near-copy match catches these (no literal anchor left); a preceding 'İ' lowercases to
  // two UTF-16 units, which must not misalign the match.
  const dpOnly = ['x <USER>.github.rossgraeber/oac-sess\n<USER>n-channels y', 'x io.git\nhub.rossgraeber/oac-s<USER>ion-channels y'];
  const dotted = ['', 'İ ', 'İİ '].flatMap((pre) => dpOnly.map((t) => ({ pre, t, ok: placeholderIntegrity(pre + t, 0).ok })));
  check('g4 sanitize (#148): a near-copy is caught with non-ASCII text before it whose lowercase is longer (İ)', dotted.every((x) => !x.ok), JSON.stringify(dotted.filter((x) => x.ok)));
  const sepSplits = ['io.', 'io.github.', 'io.github.rossgraeber/', 'io.github.rossgraeber/oac-', 'io.github.rossgraeber/oac-session-', 'io', 'io.github', 'io.github.rossgraeber', 'io.github.rossgraeber/oac', 'io.github.rossgraeber/oac-session'].flatMap((head) => ['\n', '\r\n'].flatMap((nl) => ['github', 'session', 'rossgraeber', 'oac', 'channels'].map((u) => {
    const t = `x ${head}${nl}${OAC_EXT.slice(head.length)} y`;
    return { head, nl, u, ok: placeholderIntegrity(redAs(u, sanitizeG4Text(t).text), 0).ok };
  })));
  check('g4 sanitize (#148): a wrap falling right before or after a separator (. / -) is flagged for every matching username', sepSplits.every((x) => !x.ok), JSON.stringify(sepSplits.filter((x) => x.ok).slice(0, 3)));
  const benign = [`<USER> ran /home/<USER>/x on <HOST>\n${OAC_EXT_PLACEHOLDER}`, `see github.com/<USER>/OAC (session <SECRET>)\n${OAC_EXT_PLACEHOLDER}`,`<WITHHELD: bearer token> <USER_HOME>\n${OAC_EXT_PLACEHOLDER}`];
  const benignSan = ['rossg', 'github', 'session', 'oac'].map((u) => placeholderIntegrity(redAs(u, sanitizeG4Transcript(BASELINE).text), BASELINE.split(OAC_EXT).length - 1));
  check('g4 sanitize (#148): no false alarm on placeholders that do not stand in an identifier near-copy, nor on the sanitized human-run transcript redacted for matching usernames', benign.every((t) => placeholderIntegrity(t, 1).ok) && benignSan.every((v) => v.ok), JSON.stringify([benign.map((t) => placeholderIntegrity(t, 1)), benignSan]));
  check('g4 ports: the defaults avoid the human run\'s ports, which a leftover global Codex entry may still name', !HUMAN_RUN_PORTS.includes(DEFAULT_PORTS.httpPort) && !HUMAN_RUN_PORTS.includes(DEFAULT_PORTS.modernHttpPort) && HUMAN_RUN_PORTS.includes(17448));
  check('g4 sessions: the human run had two Codex HTTP sessions on one server (two registrations), which the report\'s one-session check would flag', codexSessions(bf, 19680).length === 2);

  // --- replay: the reconstructed server against the committed fixture's own requests ------------
  const rp = await replayG4Fixture();
  const diff = (a, b) => a.map((x, i) => (x === b[i] ? null : `#${i}: fixture ${x} | reconstruction ${b[i]}`)).filter(Boolean).concat(a.length === b.length ? [] : [`length ${a.length} vs ${b.length}`]);
  check('g4 reconstruction: replaying the fixture\'s requests, every response (era, method, result or error, status, headers) matches the fixture\'s, volatile ids aside', diff(rp.base.requests, rp.run.requests).length === 0, diff(rp.base.requests, rp.run.requests).slice(0, 4).join(' || '));
  check('g4 reconstruction: the wake and relay pushes match the fixture\'s (content and meta, pids aside)', diff(rp.base.pushes, rp.run.pushes).length === 0, diff(rp.base.pushes, rp.run.pushes).slice(0, 4).join(' || '));
  check('g4 reconstruction: GET is 405 on the legacy session, as in the fixture', JSON.stringify(rp.base.gets) === JSON.stringify(rp.run.gets), JSON.stringify([rp.base.gets, rp.run.gets]));

  // --- report rules -------------------------------------------------------------------------------
  check('g4 report: only criteria 1 and 5 take an operator score, with a note', throws(() => parseG4OperatorScores([{ n: '2', score: 'equivalent', note: 'x' }]), ReportError, /mechanically/) && throws(() => parseG4OperatorScores([{ n: '5', score: 'equivalent', note: ' ' }]), ReportError, /note/) && parseG4OperatorScores([{ n: '1', score: 'not-equivalent', note: 'x' }])['1'].score === SCORES.NEQ);
  const nr = evaluateG4({ manifest: { outcome: 'NOT RUN', outcomeReason: 'timed out', scenarioData: {} }, transcriptText: null, baselineText: BASELINE, criteria: crit });
  check('g4 report: a NOT RUN leaves every criterion not evaluable', nr.rows.length === 5 && nr.rows.every((x) => x.score === SCORES.NE && /NOT RUN/.test(x.reason)));
  const tpl = renderReport({ manifest: { outcome: 'NOT RUN', scenarioData: { g4: {} } }, evaluation: nr, date: '2026-10-01', fixtures: null, runManifestName: 'x' });
  check('g4 report: not verdict-bearing, the RECONSTRUCTION stated at the top, attestation unticked, no equivalence callout', /Not verdict-bearing/.test(tpl) && /Reconstructed gate server/.test(tpl) && /never committed/.test(tpl) && /cannot be\s*\n?> verified byte-identical/.test(tpl) && (tpl.match(/^- \[ \] \*\*(?:herdr|Harness|Consent dialog):\*\*/gm) ?? []).length === 3 && !/^- \[x\]/m.test(tpl) && !/Equivalence record\*\* for G/.test(tpl));
  const V = { verified: true, cli: { claude: CPIN, codex: XPIN }, wire: { claude: CPIN, codex: XPIN }, pins: { claudeLastTested: CPIN, codexLastTested: XPIN, workingTreeMatchesHead: true } };
  const fx = { transcript: 't-herdr.jsonl', paneClaude: 'c-herdr.txt', paneCodex: 'x-herdr.txt' };
  const okRun = { outcome: 'PASS', driver: { commit: 'a'.repeat(40), toolsHerdrDirty: false }, captures: Object.values(fx).map((file) => ({ file, written: true })), scenarioData: { g4: { versions: V, postRun: { matches: true }, fixtures: fx, captureNames: fx, server: [{ match: true, workingTreeMatchesHead: true }], sanitizer: { extensionIdReplaced: 1, paneClaude: 1, paneCodex: 0 } } } };
  const texts = { transcript: `x ${OAC_EXT_PLACEHOLDER}`, paneClaude: `y ${OAC_EXT_PLACEHOLDER}`, paneCodex: 'z' };
  const with4 = (o) => ({ ...okRun, scenarioData: { g4: { ...okRun.scenarioData.g4, ...o } } });
  check('g4 report: --write refuses a capture whose placeholders did not survive redaction, or that still holds an identifier fragment', writeRefusal(okRun, texts) === null && /paneClaude capture's extension-identifier placeholders/.test(writeRefusal(okRun, { ...texts, paneClaude: 'y <#OAC_EXT_<USER>#>' })) && /identifier fragment/.test(writeRefusal(okRun, { ...texts, paneCodex: 'io.github.<USER>aeber/oac-session-channels' })) && /did not record a count/.test(writeRefusal(with4({ sanitizer: { extensionIdReplaced: 1 } }))));
  check('g4 report: --write accepts only a verified PASS from a clean, committed tools/herdr/ with the server at HEAD', writeRefusal(okRun) === null && /toolsHerdrDirty true/.test(writeRefusal({ ...okRun, driver: { commit: 'a'.repeat(40), toolsHerdrDirty: true } })) && /only a PASS run/.test(writeRefusal({ ...okRun, outcome: 'FAIL' })) && /committed source/.test(writeRefusal(with4({ server: [{ match: true, workingTreeMatchesHead: false }] }))));
  const midRun = with4({ postRun: { matches: false } });
  const disagree = with4({ versions: { ...V, wire: { claude: CPIN, codex: '0.158.0' } } });
  check('g4 report (#216 operator decision): a mid-run version change or a CLI/wire disagreement never refuses --write; the fixtures are withheld with a VERSION WARNING', writeRefusal(midRun) === null && writeRefusal(disagree) === null && /^VERSION WARNING: .*no fixture is added/.test(fixtureWithheld(midRun)) && /^VERSION WARNING: .*no fixture is added/.test(fixtureWithheld(disagree)) && fixtureWithheld(okRun) === null && writeRefusal(with4({ versions: disagree.scenarioData.g4.versions, sanitizer: {} })) === null);
  const drifted = { ...V, cli: { claude: '2.1.999', codex: '0.999.0' }, wire: { claude: '2.1.999', codex: '0.999.0' } };
  check('g4 report (#216): a drifted but consistent version is not refused by --write; it only does not match the last tested versions', writeRefusal(with4({ versions: drifted })) === null && versionMatchesLastTested(okRun.scenarioData.g4) && !versionMatchesLastTested(with4({ versions: drifted }).scenarioData.g4) && writeRefusal(with4({ versions: { ...V, pins: { ...V.pins, workingTreeMatchesHead: false } } })) === null);
}

// --- lifecycle cases ------------------------------------------------------------------------------

const today = () => new Date().toISOString().slice(0, 10);
const FAST = ['--param', 'settleMs=300', '--param', 'pollMs=200', '--param', 'wireTimeoutMs=10000', '--param', 'turnTimeoutMs=30000', '--param', 'startupTimeoutMs=20000', '--param', 'handshakeTimeoutMs=20000'];
const PORTS = (a, b) => ['--param', `httpPort=${a}`, '--param', `modernHttpPort=${b}`];
const inside = (p, root) => {
  const rel = relative(root, p);
  return rel === '' || (!rel.startsWith('..') && !isAbsolute(rel));
};

export function g4Cases(check) {
  const cases = [];
  const run = (name, opts, assert, setup) => cases.push({ name, opts: { scenario: 'g4-mcp-dual-era', mode: 'fake-claude,fake-codex', fakeClaude: {}, ...opts }, assert, setup });
  const names = () => fixtureNames(today(), CPIN, XPIN);
  const { criteria: crit } = readG4Criteria(REPO);
  const evalRun = (r, operatorScores = {}) => evaluateG4({ manifest: r.manifest, transcriptText: r.capture(names().transcript), paneClaudeText: r.capture(names().paneClaude), baselineText: BASELINE, criteria: crit, operatorScores });

  run('g4 human accept (traced)', { args: ['--param', 'accept=human', ...FAST, ...PORTS(37448, 37450)], fakeClaude: { FAKE_CLAUDE_SELF_ACCEPT_MS: '1000' }, fakeCodex: { FAKE_CODEX_SELF_ACCEPT_MS: '1000', trace: true } }, (r) => {
    const m = r.manifest;
    const g4 = m.scenarioData.g4;
    check('g4 human: PASS (exit 0)', r.status === 0 && m.outcome === 'PASS', `${r.status} ${m.outcome} ${m.outcomeReason}`);
    check('g4 human: Claude launched with G4\'s verbatim argv, MCP_SDK_GENERATION=v2 in its pane env', r.calls.some((c) => /agent start g4claude --kind claude --pane w1:p1 --timeout \d+ -- --dangerously-load-development-channels server:g4spike server:g4modern$/.test(c.argv.join(' '))) && r.calls.some((c) => c.argv.includes('--env') && c.argv.includes('MCP_SDK_GENERATION=v2')));
    check('g4 human: Codex launched per invocation (`-c mcp_servers.g4http.url=...`), its pane argv matching the validated overrides', r.calls.some((c) => /agent start g4codex --kind codex --pane w2:p1 --timeout \d+ -- -c mcp_servers\.g4http\.url="http:\/\/127\.0\.0\.1:37448\/mcp"$/.test(c.argv.join(' '))) && g4.codexLaunch.paneArgv.matchesLaunch === true && g4.codexLaunch.validation.ok);
    check('g4 human: both dialogs read verbatim and accepted outside the driver', g4.dialogs.length === 2 && g4.dialogs.every((d) => d.acceptOrigin === 'human' && d.inputBetweenReadAndAccept === 0) && !m.commands.some((x) => x.role === 'dialog-accept'));
    check('g4 human: CLI and wire versions verified for both harnesses; captures carry the K8 fixture names, written clean', g4.versions.verified && g4.versions.wire.claude === CPIN && g4.versions.wire.codex === XPIN && m.captures.map((c) => c.file).sort().join() === Object.values(names()).sort().join() && m.captures.every((c) => c.written && c.redaction.residualLeaks.length === 0 && c.redaction.residualGenericHits.length === 0));
    check('g4 human: herdr typed only the three operator prompts; no notification went through herdr', r.prompts.length === 3 && r.prompts.map((p) => p.text).join('|') === [DEFAULT_PROMPTS.claudeEchoPrompt, DEFAULT_PROMPTS.codexToolsPrompt, DEFAULT_PROMPTS.claudeEchoAfterPrompt].join('|') && m.commands.every((x) => !x.argv.some((a) => /notifications\/claude\/channel|G4 wake test|relayed from http/.test(a))));
    const ev = evalRun(r);
    check('g4 human: report scores C2, C3, C4 equivalent; C1 and C5 pending the operator with every precondition met', ev.rows.map((x) => x.score).join('|') === [SCORES.NE, SCORES.EQ, SCORES.EQ, SCORES.EQ, SCORES.NE].join('|') && /operator review/.test(ev.rows[0].reason) && /operator review/.test(ev.rows[4].reason), JSON.stringify(ev.rows.map((x) => [x.score, x.reason])));
    check('g4 human: operator scores for C1 and C5 apply once their preconditions hold', evalRun(r, parseG4OperatorScores([{ n: '1', score: 'equivalent', note: 'pane #x' }, { n: '5', score: 'equivalent', note: 'pane #y' }])).rows.every((x) => x.score === SCORES.EQ));
    check('g4 human: the extension identifier reached the capture only as its placeholder', !r.capture(names().transcript).includes(OAC_EXT) && r.capture(names().transcript).includes(OAC_EXT_PLACEHOLDER) && g4.sanitizer.extensionIdReplaced > 0);
    check('g4 human: every capture was sanitized (counts recorded for the transcript and both panes) and its placeholders survived redaction', [['transcript', g4.sanitizer.extensionIdReplaced], ['paneClaude', g4.sanitizer.paneClaude], ['paneCodex', g4.sanitizer.paneCodex]].every(([k, n]) => Number.isInteger(n) && placeholderIntegrity(r.capture(names()[k]), n).ok), JSON.stringify(g4.sanitizer));
    check('g4 human: exactly one Codex HTTP session, no multiple-registration finding', g4.codex.sessions.length === 1 && !m.findings.some((f) => /Codex HTTP MCP sessions/.test(f)));

    // Credential hygiene and the no-global-config rule, from the trace.
    const trace = read(join(r.base, 'fs-trace.jsonl')).split('\n').filter(Boolean).map((l) => JSON.parse(l));
    const homes = [r.env.CODEX_HOME, r.env.CLAUDE_CONFIG_DIR].flatMap((h) => { try { return [h, realpathSync(h)]; } catch { return [h]; } });
    const underHome = (t) => homes.some((h) => inside(t.path, h));
    const RUN_JS = join(REPO, 'tools', 'herdr', 'run.mjs');
    const driver = trace.filter((t) => t.script && resolve(t.script) === RUN_JS);
    const server = trace.filter((t) => t.script && /[\\/]g4-server[\\/]g4-server\.mjs$/.test(t.script));
    const hashed = new Set(['config.toml', 'hooks.json'].flatMap((n) => [join(r.env.CODEX_HOME, n), join(realpathSync(r.env.CODEX_HOME), n)]).concat([join(r.env.CLAUDE_CONFIG_DIR, 'settings.json'), join(realpathSync(r.env.CLAUDE_CONFIG_DIR), 'settings.json')]));
    check('g4 trace: the tracer saw the driver and the reconstructed server', driver.length > 50 && server.length > 0, `${driver.length} ${server.length}`);
    check('g4 trace: nothing traced wrote, moved or deleted anything under either harness home', trace.filter((t) => t.kind === 'write' && t.path && underHome(t)).length === 0, JSON.stringify(trace.filter((t) => t.kind === 'write' && t.path && underHome(t)).map((t) => t.path)));
    check('g4 trace: no process wrote a Codex config file anywhere (no global edit, no project .codex/config.toml); positive control: the driver\'s own herdr-config.toml write IS traced', trace.filter((t) => t.kind === 'write' && t.path && basename(t.path) === 'config.toml').length === 0 && trace.some((t) => t.kind === 'write' && basename(t.path ?? '') === 'herdr-config.toml'));
    check('g4 trace: nothing copied or listed the Codex home (never copied)', trace.filter((t) => ['copyFileSync', 'copyFile', 'cpSync', 'cp', 'readdirSync', 'readdir', 'opendirSync', 'opendir'].includes(t.op) && t.path && underHome(t)).length === 0);
    check('g4 trace: the driver read nothing under either home beyond the three harness-config hashes', driver.filter((t) => t.kind === 'fs' && t.path && underHome(t)).every((t) => hashed.has(t.path)), JSON.stringify([...new Set(driver.filter((t) => t.kind === 'fs' && t.path && underHome(t) && !hashed.has(t.path)).map((t) => t.path))]));
    check('g4 trace: the server opened nothing under either home and started no process', server.filter((t) => t.path && underHome(t)).length === 0 && server.filter((t) => t.kind === 'spawn').length === 0);

    // The report CLI.
    const draft = spawnSync(process.execPath, [REPORT, '--run', r.outDir], { encoding: 'utf8', timeout: 20000 });
    check('g4 report CLI: draft printed with the reconstruction callout, the method difference and the pinned criteria source', draft.status === 0 && /Reconstructed gate server/.test(draft.stdout) && /GLOBAL Codex config/.test(draft.stdout) && new RegExp(`Criteria source:.*G4-mcp-dual-era\\.md.*${G4_CRITERIA_SHA256}`).test(draft.stdout) && /## Exchange counts/.test(draft.stdout), draft.stderr);
    const badScore = spawnSync(process.execPath, [REPORT, '--run', r.outDir, '--score', '3=equivalent', '--note', '3=trust me'], { encoding: 'utf8', timeout: 20000 });
    check('g4 report CLI: refuses an operator score for a mechanically scored criterion', badScore.status === 2 && /mechanically/.test(badScore.stderr));
    const root = mkdtempSync(join(tmpdir(), 'oac-g4-report-'));
    try {
      const w = spawnSync(process.execPath, [REPORT, '--run', r.outDir, '--write', '--root', root], { encoding: 'utf8', timeout: 20000 });
      if (m.driver.toolsHerdrDirty === false && g4.server.every((s) => s.workingTreeMatchesHead)) {
        const date = g4.date;
        const expected = [`docs/planning/gates/herdr-runs/G4-${date}.md`, `docs/planning/gates/herdr-runs/G4-${date}.run-manifest.json`, ...Object.values(names()).map((f) => `${FIXTURE_DIR}/${f}`)];
        check('g4 report CLI --write (clean tools/herdr/): report, run manifest and all three fixtures written under the root', w.status === 0 && expected.every((p) => existsSync(join(root, p))), w.stdout + w.stderr);
        const entries = JSON.parse(read(join(r.outDir, 'manifest-entries.draft.json')));
        const REQUIRED = ['path', 'provider', 'surface', 'observed_version', 'pins_row', 'pins_as_of', 'version_matches_pin', 'capture_date', 'capture_utc_range', 'superseded_by', 'redaction', 'coverage'];
        check('g4 report CLI --write: draft entries carry every required field, the driver block, and a schema block on each Codex-touching entry', entries.length === 3 && entries.every((e) => REQUIRED.every((k) => k in e) && e.version_matches_pin === true && /^[0-9a-f]{40}$/.test(e.driver.driver_commit) && e.driver.run_manifest === `docs/planning/gates/herdr-runs/G4-${date}.run-manifest.json` && (!e.provider.includes('codex') || e.schema?.applicable === false)) && entries.every((e) => /RECONSTRUCTION/.test(e.notes[0])));
        const again = spawnSync(process.execPath, [REPORT, '--run', r.outDir, '--write', '--root', root], { encoding: 'utf8', timeout: 20000 });
        check('g4 report CLI --write: never overwrites', again.status === 2 && /refusing to overwrite/.test(again.stderr));
      } else {
        check('g4 report CLI --write (tools/herdr/ dirty or the server not at HEAD in this checkout): refused, nothing written', w.status === 2 && /--write refused/.test(w.stderr) && readdirSync(root).length === 0 && !existsSync(join(r.outDir, 'manifest-entries.draft.json')), w.stderr);
      }
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
    const entries = draftManifestEntries({ manifest: m, fixtures: names(), runManifestPath: 'docs/planning/gates/herdr-runs/G4-x.run-manifest.json', transcriptText: r.capture(names().transcript), pinsCommit: 'p', redactSha256: 'r' });
    check('g4 draft entries (built directly, whatever the checkout state): Codex-touching entries carry an applicable:false schema; the transcript\'s coverage names the Claude-modern and Codex-legacy calls', entries[0].schema.applicable === false && entries[2].schema.applicable === false && !('schema' in entries[1]) && !!entries[0].coverage['tools/call (Claude HTTP-modern)'] && !!entries[0].coverage['tools/call (Codex HTTP legacy)']);
  });

  // #199: the fake Codex shows its trust dialog as seen live on 0.159.2; the driver accepts it
  // (enter) as it does Claude Code's dev-channels dialog.
  run('g4 driver accept', { args: ['--param', 'accept=driver', ...FAST, ...PORTS(37458, 37460)], fakeCodex: {} }, (r) => {
    const m = r.manifest;
    const g4 = m.scenarioData.g4;
    const cx = g4.dialogs.find((d) => d.agent === 'codex');
    check('g4 driver #199: the Codex trust dialog was accepted by the driver with `enter` alone', cx?.kind === 'workspace-trust' && JSON.stringify(cx.acceptKeys?.map((k) => k.key)) === '["enter"]', JSON.stringify(g4.dialogs));
    check('g4 driver: PASS; each recognized dialog read, then accepted by the driver with no input in between', r.status === 0 && g4.dialogs.length === 2 && g4.dialogs.map((d) => d.agent).sort().join() === 'claude,codex' && g4.dialogs.every((d) => d.acceptOrigin === 'driver' && d.inputBetweenReadAndAccept === 0 && m.commands.find((x) => x.seq === d.acceptSeq - 1)?.argv.includes('read')), `${m.outcome} ${m.outcomeReason}`);
    check('g4 driver: the report still scores nothing on the accept (no G4 criterion names it)', evalRun(r).rows.map((x) => x.score).join('|') === [SCORES.NE, SCORES.EQ, SCORES.EQ, SCORES.EQ, SCORES.NE].join('|'));
  });

  // #244: a codexLaunch the allowlist refuses is refused by the scenario's validateParams before
  // run.mjs creates scratch, an output directory or a manifest: usage error (exit 2), nothing
  // recorded, and the console reason never quotes the refused text.
  const SECRET = 'S3CRETvalue42';
  const refusedBeforeRun = (name, codexLaunchArgv, port) => cases.push({
    name,
    opts: { scenario: 'g4-mcp-dual-era', mode: 'fake-claude,fake-codex', fakeClaude: {}, args: [...FAST, ...PORTS(port, port + 2), '--param', `codexLaunch=${JSON.stringify(codexLaunchArgv)}`], fakeCodex: { FAKE_CODEX_DIALOG: 'none' } },
    invariantOpts: { noManifest: true },
    assert: (r) => check(`${name}: usage error (exit 2) naming the rule, the refused text on no output`, r.status === 2 && /codexLaunch refused: .*refused before anything was created, nothing recorded/.test(r.stderr) && !`${r.stdout}${r.stderr}`.includes(SECRET), `${r.status} ${r.stderr}`),
  });
  refusedBeforeRun('g4 codex launch with a non-MCP override', ['codex', '-c', `model="${SECRET}"`, '-c', 'mcp_servers.g4http.url="http://127.0.0.1:37468/mcp"'], 37468);
  refusedBeforeRun('g4 codex launch #244: userinfo URL', ['codex', '-c', `mcp_servers.g4http.url="http://op:${SECRET}@127.0.0.1:37528/mcp"`], 37528);
  refusedBeforeRun('g4 codex launch #244: non-loopback URL', ['codex', '-c', `mcp_servers.g4http.url="http://${SECRET}.example.net:37538/mcp"`], 37538);

  run('g4 launch not verbatim', { args: [...FAST, '--launch', '["claude","--dangerously-load-development-channels","server:g4spike"]'], fakeCodex: { FAKE_CODEX_DIALOG: 'none' } }, (r) => {
    check('g4 launch: a launch other than G4\'s verbatim one FAILs before anything starts', r.status === 1 && /not G4's verbatim launch/.test(r.manifest.outcomeReason) && !r.calls.some((c) => c.argv.includes('agent')));
  });

  // #216: versions float. A version other than PINS.md's last tested one, on either harness's
  // CLI or wire, is a VERSION WARNING finding and the run proceeds.
  run('g4 drifted versions on both harnesses warn and the run proceeds', { args: ['--param', 'accept=driver', ...FAST, ...PORTS(37478, 37480)], fakeClaude: { FAKE_CLAUDE_CLI_VERSION: '2.1.999', FAKE_CLAUDE_VERSION: '2.1.999' }, fakeCodex: { FAKE_CODEX_DIALOG: 'none', FAKE_CODEX_VERSION: '0.999.0' } }, (r) => {
    const m = r.manifest;
    const g4 = m.scenarioData.g4;
    check('g4 #216 drift: PASS (exit 0), never NOT RUN on a version', r.status === 0 && m.outcome === 'PASS', `${r.status} ${m.outcome} ${m.outcomeReason}`);
    check('g4 #216 drift: four VERSION WARNING findings naming G4 (both CLIs, both wires)', g4.versions.warnings.length === 4 && m.findings.filter((f) => /^VERSION WARNING \(G4\)/.test(f)).length === 4, JSON.stringify(m.findings));
    check('g4 #216 drift: Codex was prompted, captures name the observed versions, matchesLastTested false', r.prompts.some((p) => p.target === 'g4codex') && g4.versions.verified === true && g4.versions.matchesLastTested === false && JSON.stringify(g4.fixtures) === JSON.stringify(fixtureNames(today(), '2.1.999', '0.999.0')), JSON.stringify(g4.captureNames));
  });

  run('g4 Codex MCP wire version differs from its CLI: warns, proceeds, captures stay unverified', { args: ['--param', 'accept=driver', ...FAST, ...PORTS(37488, 37490)], fakeCodex: { FAKE_CODEX_DIALOG: 'none', FAKE_CODEX_MCP_VERSION: '0.999.0' } }, (r) => {
    const m = r.manifest;
    const g4 = m.scenarioData.g4;
    check('g4 #216 Codex wire != CLI: not stopped on the version; Codex was prompted; a VERSION WARNING from the MCP client user-agent', m.outcome !== 'NOT RUN' && r.prompts.some((p) => p.target === 'g4codex') && m.findings.some((f) => /^VERSION WARNING \(G4\): the MCP client user-agent \(codex-mcp-client\/<version>\) reports 0\.999\.0/.test(f)), `${m.outcome} ${m.outcomeReason}`);
    check('g4 #216 Codex wire != CLI: captures unverified-* only', g4.fixtures === null && g4.versions.verified === false && m.captures.length > 0 && m.captures.every((c) => c.file.startsWith('unverified-')), JSON.stringify(m.captures.map((c) => c.file)));
    const w = spawnSync(process.execPath, [REPORT, '--run', r.outDir, '--write', '--root', join(r.base, 'nowrite')], { encoding: 'utf8', timeout: 20000 });
    const recDate = g4.date;
    const rec = join(join(r.base, 'nowrite'), `docs/planning/gates/herdr-runs/G4-${recDate}.md`);
    if (m.driver.toolsHerdrDirty === false) {
      check('g4 #216 Codex wire != CLI: --write writes the record and run manifest with a VERSION WARNING, but no fixture and no MANIFEST draft (operator decision on #216)', w.status === 0 && existsSync(rec) && existsSync(join(join(r.base, 'nowrite'), `docs/planning/gates/herdr-runs/G4-${recDate}.run-manifest.json`)) && !existsSync(join(join(r.base, 'nowrite'), FIXTURE_DIR)) && !existsSync(join(r.outDir, 'manifest-entries.draft.json')) && /No fixture written: VERSION WARNING/.test(w.stdout) && /Finding: VERSION WARNING: .*no fixture is added/.test(readFileSync(rec, 'utf8')), w.stdout + w.stderr);
    } else {
      check('g4 #216 Codex wire != CLI: --write is refused only for the dirty tools/herdr/, never for the versions', w.status === 2 && /toolsHerdrDirty/.test(w.stderr) && !/same .*version/.test(w.stderr), w.stderr);
    }
  });

  run('g4 a second Codex registration also connects', { args: ['--param', 'accept=driver', ...FAST, ...PORTS(37508, 37510), '--param', 'codexLaunch=["codex","-c","mcp_servers.g4http.url=\\"http://127.0.0.1:37508/mcp\\"","-c","mcp_servers.leftover.url=\\"http://127.0.0.1:37508/mcp\\""]'], fakeCodex: { FAKE_CODEX_DIALOG: 'none' } }, (r) => {
    const m = r.manifest;
    check('g4 two registrations: the run records a finding that the Codex traffic is not attributable to one registration', m.findings.some((f) => /2 Codex HTTP MCP sessions/.test(f) && /cannot be attributed/.test(f)) && m.scenarioData.g4.codex.sessions.length === 2, JSON.stringify(m.findings));
    const c4 = evalRun(r).rows[3];
    check('g4 two registrations: criterion 4 is not equivalent on the one-session check (never silently credited)', c4.score === SCORES.NEQ && /exactly one Codex HTTP MCP session/.test(c4.reason), c4.reason);
  });

  run('g4 the human run\'s port', { args: [...FAST, ...PORTS(17448, 37520)], fakeCodex: { FAKE_CODEX_DIALOG: 'none' } }, (r) => {
    check('g4 human-run port: refused before anything starts (a leftover global registration may name it)', r.status === 1 && /the human G4 run's/.test(r.manifest.outcomeReason) && !r.calls.some((c) => c.argv.includes('agent')), r.manifest.outcomeReason);
  });

  let holder = null;
  run('g4 port already in use', { args: [...FAST, ...PORTS(37498, 37500)], fakeCodex: { FAKE_CODEX_DIALOG: 'none' } }, (r) => {
    check('g4 port busy: NOT RUN before anything launches (a stale server must not answer for this run)', r.status === 3 && /127\.0\.0\.1:37498 is not free/.test(r.manifest.outcomeReason) && !r.calls.some((c) => c.argv.includes('agent')), r.manifest.outcomeReason);
  }, () => new Promise((res) => {
    holder = createServer();
    holder.listen(37498, '127.0.0.1', () => res(() => new Promise((done) => holder.close(() => done()))));
  }));
  return cases;
}
