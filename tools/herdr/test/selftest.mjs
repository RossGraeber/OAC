// Self-test for the herdr driver: `node tools/herdr/run.mjs --self-test`.
//
// Unit checks run in-process (pin parsing, quoting, redaction, guards, bounded children).
// Lifecycle checks run tools/herdr/run.mjs end to end against test/fake-herdr.mjs -- a test
// double of the herdr CLI, not herdr -- so they prove the DRIVER's behavior (session
// lifecycle, timeouts ending NOT RUN, teardown on every path, manifest content) with no
// herdr installed. Nothing here proves anything about herdr itself; the live smoke run
// against a real herdr is a separate, operator-run step.
//
// POSIX only for the lifecycle half (the fake runs pane commands with `sh`).

import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir, homedir } from 'node:os';
import { dirname, join, resolve, relative, isAbsolute } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';

import { parseHerdrPin, readHerdrPin, versionMatches } from '../lib/pins.mjs';
import { quoteCommand, regexLiteral } from '../lib/pane-shell.mjs';
import { createRedactor, reportIsClean, parseLiteralSpec } from '../lib/redact.mjs';
import { HerdrSession, DriverError, ROLES, isHerdrWait, isInputCommand, makeSessionName } from '../lib/herdr.mjs';
import { harnessConfigFiles, herdrLaunchEnv } from '../lib/manifest.mjs';
import { runBounded, isAlive, processesForSession } from '../lib/proc.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..', '..');
const RUN = join(HERE, '..', 'run.mjs');
const FAKE = join(HERE, 'fake-herdr.mjs');
const BOX_C = join(REPO, 'docs', 'planning', 'gates', 'fixtures', 'g1-claude-wake', 'transcript-2026-09-28-2.1.283-boxC.jsonl');

let passed = 0;
let failed = 0;
function check(name, cond, detail = '') {
  if (cond) {
    passed += 1;
    console.log(`  ok    ${name}`);
  } else {
    failed += 1;
    console.log(`  FAIL  ${name}${detail ? `: ${detail}` : ''}`);
  }
}
const sha = (s) => createHash('sha256').update(s).digest('hex');
const within = (dir, root) => {
  const rel = relative(root, dir);
  return rel !== '' && !rel.startsWith('..') && !isAbsolute(rel);
};

// --- unit ------------------------------------------------------------------------------

function unitPins() {
  const pin = readHerdrPin(join(REPO, 'docs', 'planning', 'PINS.md'));
  check('pins: PINS.md "herdr (test tooling)" row parses to a release tag', /^v\d+\.\d+\.\d+$/.test(pin.tag), pin.tag);
  check('pins: expected --version output is `herdr <tag without v>`', pin.expectedVersionOutput === `herdr ${pin.tag.slice(1)}`);
  const table = (rows) => ['| Surface | Stability label | Pinned version | Gates affected |', '|---|---|---|---|', ...rows].join('\n');
  const p = parseHerdrPin(table(['| herdr (test tooling) | supported | `v1.2.3` (tag object `abc`); fixed | none |']));
  check('pins: synthetic row -> v1.2.3', p.tag === 'v1.2.3' && p.version === '1.2.3');
  check('pins: exact stable output matches', versionMatches(p, 'herdr 1.2.3\n'));
  check('pins: bare version refused', !versionMatches(p, '1.2.3'));
  check('pins: preview build of the same base refused', !versionMatches(p, 'herdr 1.2.3-preview.abc'));
  check('pins: other version refused', !versionMatches(p, 'herdr 1.2.4'));
  const throws = (text) => {
    try {
      parseHerdrPin(text);
      return false;
    } catch {
      return true;
    }
  };
  check('pins: missing row throws', throws(table(['| zenoh | supported | `1.10.1` | G3 |'])));
  check('pins: duplicate row throws', throws(table(['| herdr (test tooling) | s | `v1.2.3` | none |', '| herdr (test tooling) | s | `v1.2.4` | none |'])));
  check('pins: unparseable tag throws', throws(table(['| herdr (test tooling) | s | **floating** | none |'])));
}

function unitQuoting() {
  check('quote: posix plain', quoteCommand(['echo', 'OAC-SMOKE-READY']) === 'echo OAC-SMOKE-READY');
  check('quote: posix spaces and quotes', quoteCommand(['/p a/node', "it's"]) === `'/p a/node' 'it'\\''s'`);
  check('quote: powershell quoted command gets &', quoteCommand(['C:\\Program Files\\node.exe', 'x y'], 'powershell') === "& 'C:\\Program Files\\node.exe' 'x y'");
  check('quote: regex literal escapes metacharacters', regexLiteral('a.b(c)') === 'a\\.b\\(c\\)');
}

function unitRedaction() {
  const boxC = readFileSync(BOX_C, 'utf8');
  for (const [label, r] of [
    ['this machine', createRedactor()],
    ['synthetic identity', createRedactor({ home: '/home/alice', username: 'alice', hostname: 'buildbox-7' })],
    ['user root', createRedactor({ home: '/root', username: 'root', hostname: 'vm' })],
  ]) {
    const scan = r.scan(boxC);
    check(`redact: Box C check-only has zero residual hits (${label})`, scan.residualLeaks.length === 0 && scan.residualGenericHits.length === 0, JSON.stringify(scan));
    const red = r.redactJsonl(boxC);
    check(`redact: Box C redaction is clean and byte-identical (${label})`, reportIsClean(red.report) && red.report.droppedHazardLines === 0 && red.text === boxC, JSON.stringify(red.report));
  }

  const r = createRedactor({
    home: '/home/alice',
    username: 'alice',
    hostname: 'buildbox-7.corp.example',
    literals: [{ value: '/tmp/oac-herdr-scratch-Zq9', placeholder: '<SCRATCH>' }],
  });
  const secrets = {
    sk: `sk-ant-api03-${'Q'.repeat(24)}`,
    gh: `ghp_${'a1B2'.repeat(9)}`,
    jwt: `eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.${'s'.repeat(20)}`,
    bearer: `Bearer ${'t0k3n'.repeat(5)}`,
    aws: `AKIA${'Z'.repeat(16)}`,
    assign: `MY_SERVICE_TOKEN=${'v'.repeat(18)}`,
  };
  const dirty = [
    'alice@buildbox-7:~/work$ ls /home/alice/projects/oac',
    'cwd C:\\\\Users\\\\bob\\\\AppData\\\\Local\\\\Temp and C:\\Users\\bob\\x',
    'claude project dir -home-alice-OAC and /Users/carol/src',
    `token ${secrets.sk} and ${secrets.gh} and ${secrets.aws}`,
    `jwt ${secrets.jwt}`,
    `header-free ${secrets.bearer}`,
    `env ${secrets.assign}`,
    'contact alice.smith@example.com about buildbox-7.corp.example',
    'scratch /tmp/oac-herdr-scratch-Zq9/captures/pane.txt',
    '-----BEGIN OPENSSH PRIVATE KEY-----',
    'b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW',
    '-----END OPENSSH PRIVATE KEY-----',
    'Authorization: Basic Zm9vOmJhcg==',
    'clean line: capabilities {"roots":{"listChanged":true}}',
  ].join('\n');

  const pre = r.scan(dirty);
  const preLeakLabels = new Set(pre.residualLeaks.map((h) => h.label));
  const preGeneric = new Set(pre.residualGenericHits.map((h) => h.label));
  check('redact: scan catches home, username, hostname, literal on unredacted text', ['home path', 'username', 'hostname', 'literal <SCRATCH>'].every((l) => preLeakLabels.has(l)), [...preLeakLabels].join(','));
  check(
    'redact: scan catches generic shapes on unredacted text',
    ['sk- token', 'github token', 'jwt', 'bearer token', 'aws access key id', 'secret assignment', 'email', 'windows profile path', 'unix home path', 'mangled home path', 'private key block', 'authorization header'].every((l) => preGeneric.has(l)),
    [...preGeneric].join(','),
  );
  const reportText = JSON.stringify(pre);
  check('redact: a scan report never contains matched values', !Object.values(secrets).some((s) => reportText.includes(s)) && !reportText.includes('alice') && !reportText.includes('buildbox'));

  const { text, report } = r.redactText(dirty);
  check('redact: synthetic hazards redacted to zero residual hits', reportIsClean(report), JSON.stringify(report));
  check('redact: key block (3 lines) and header line dropped', report.droppedHazardLines === 4, String(report.droppedHazardLines));
  check('redact: no secret survives', !Object.values(secrets).some((s) => text.includes(s.replace(/^Bearer /, ''))));
  check('redact: no identity survives', !/alice|buildbox|bob|carol|oac-herdr-scratch/i.test(text), text);
  check('redact: placeholders present', ['<USER_HOME>', '<USER>', '<HOST>', '<EMAIL>', '<SECRET>', '<SCRATCH>', '<USER_HOME_MANGLED>'].every((p) => text.includes(p)), text);
  check('redact: secret assignment keeps the variable name', text.includes('MY_SERVICE_TOKEN=<SECRET>'));
  check('redact: clean line untouched', text.includes('clean line: capabilities {"roots":{"listChanged":true}}'));
  check('redact: second pass is a no-op', r.redactText(text).text === text);

  const rootR = createRedactor({ home: '/root', username: 'root', hostname: 'vm' });
  const rootText = rootR.redactText('{"roots":{"listChanged":true}} root@vm:/root/x# cd /root').text;
  check('redact: common username "root" only replaced in context', rootText === '{"roots":{"listChanged":true}} <USER>@<HOST>:<USER_HOME>/x# cd <USER_HOME>', rootText);

  const val = r.redactValue({ argv: ['herdr', '--cwd', '/home/alice/x'], nested: { k: secrets.sk }, key: '-----BEGIN RSA PRIVATE KEY-----' });
  check('redact: structured values redacted in place', val.value.argv[2] === '<USER_HOME>/x' && val.value.nested.k === '<SECRET>' && val.value.key === '<REDACTED_HAZARD_LINE>' && reportIsClean(val.report));

  // Fix 2: a private key is dropped BEGIN through END as one unit; unterminated fails closed.
  const body = ['b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW', 'QyNTUxOQAAACDSyntheticBodyLineTwo', 'AAAEDSyntheticBodyLineThree'];
  const keyText = ['before', '-----BEGIN OPENSSH PRIVATE KEY-----', ...body, '-----END OPENSSH PRIVATE KEY-----', 'after'].join('\n');
  const k = r.redactText(keyText);
  check('fix2: multi-line private key body and END line do not survive text redaction', k.text === 'before\nafter' && k.report.droppedHazardLines === 5 && reportIsClean(k.report), JSON.stringify(k));
  const ku = r.redactText(['before', '-----BEGIN RSA PRIVATE KEY-----', ...body, 'trailing'].join('\n'));
  check('fix2: unterminated key block drops everything after BEGIN (fail closed)', ku.text === 'before' && ku.report.unterminatedKeyBlock === true);
  const kj = r.redactJsonl(`{"a":1}\n-----BEGIN PGP PRIVATE KEY BLOCK-----\n${body.join('\n')}\n-----END PGP PRIVATE KEY BLOCK-----\n{"b":2}\n`);
  check('fix2/11: JSONL capture gets the same key-block treatment', kj.text === '{"a":1}\n{"b":2}\n' && reportIsClean(kj.report), JSON.stringify(kj));
  check('fix2: a stray END marker or key-material line is a residual hit', r.scan('-----END OPENSSH PRIVATE KEY-----').residualGenericHits.length > 0 && r.scan(body[0]).residualGenericHits.length > 0);

  // Fix 3: JSON-shaped auth headers.
  const hdr = '{"headers":{"Authorization":"Basic c2VjcmV0OnNlY3JldA=="}}\n{"Cookie":"session=abc123def456"}\n{"Set-Cookie": "sid=abc123def456"}\nclean';
  check('fix3: JSON-shaped Authorization/Cookie headers are residual hits before redaction', r.scan(hdr).residualGenericHits.filter((h) => /authorization|cookie/.test(h.label)).length === 3);
  const hr = r.redactText(hdr);
  check('fix3: JSON-shaped header lines dropped in text mode', hr.text === 'clean' && hr.report.droppedHazardLines === 3 && reportIsClean(hr.report), JSON.stringify(hr));
  const hj = r.redactJsonl('{"t":1,"headers":{"Authorization":"Basic c2VjcmV0OnNlY3JldA==","Cookie":"session=abc123def456"}}\n');
  check('fix3: JSONL header values redacted by key', hj.text === '{"t":1,"headers":{"Authorization":"<SECRET>","Cookie":"<SECRET>"}}\n' && reportIsClean(hj.report), JSON.stringify(hj));

  // Finding 4: secret-named keys in any case or spelling.
  const secretLines = [
    '{"access_token": "abcdef123456"}',
    '{"apiKey":"abcdef123456"}',
    'aws_secret_access_key = wJalrXUtnFEMIK7MDENG',
    'x-api-key: abcdef123456',
    'run --api-key abcdef123456 now',
    'TOKEN=abcdef123456',
    'SECRET=abcdef123456',
    `npm token npm_${'a1B2c3D4e5'.repeat(3)}a1B2c3`,
    `glpat-${'x'.repeat(20)}`,
  ];
  const sr = r.redactText(secretLines.join('\n'));
  check('finding4: secret-key shapes are residual hits before redaction', secretLines.every((l) => r.scan(l).residualGenericHits.length > 0), secretLines.filter((l) => r.scan(l).residualGenericHits.length === 0).join(' | '));
  check('finding4: secret-key shapes redacted, names kept', !/abcdef123456|wJalrXUtnFEMI|npm_a1|glpat-x/.test(sr.text) && sr.text.includes('"access_token": "<SECRET>"') && sr.text.includes('--api-key <SECRET>') && reportIsClean(sr.report), sr.text);
  const benign = '{"progressToken":"abc123def456"} password: \nTOKEN=${MY_TOKEN} max_tokens=4096';
  check('finding4: bookkeeping names, empty values, and variable references untouched', r.redactText(benign).text === benign, r.redactText(benign).text);

  // Finding 5: identity past word boundaries, after ANSI codes, WSL and percent-encoded paths.
  const ir = createRedactor({ home: '/home/rossg', username: 'rossg', hostname: 'Ross-MBP_2' });
  const idText = 'rossg_dev rossg2 xrossg on Ross-MBP_2 and ross-mbp_2.local';
  const it = ir.redactText(idText);
  check('finding5: username/hostname inside longer tokens redacted', !/rossg|ross-mbp/i.test(it.text) && reportIsClean(it.report), it.text);
  check('finding5: identity inside longer tokens is a residual hit before redaction', ir.scan(idText).residualLeaks.length >= 2);
  const rootR2 = createRedactor({ home: '/root', username: 'root', hostname: 'vm' });
  check('finding5: username after an ANSI colour code', rootR2.redactText('\x1b[01;32mroot@vm\x1b[00m:~#').text === '\x1b[01;32m<USER>@<HOST>\x1b[00m:~#');
  const pathsText = 'wsl /mnt/c/Users/dave/x and file:///%2Fhome%2Fcarol%2Fsrc and C%3A%5CUsers%5Cerin%5Cx';
  const pt = r.redactText(pathsText);
  check('finding5: WSL and percent-encoded home paths redacted', !/dave|carol|erin/.test(pt.text) && reportIsClean(pt.report), pt.text);
  check('finding5: WSL and percent-encoded home paths are residual hits before redaction', r.scan(pathsText).residualGenericHits.length >= 3);

  // Finding 6: run-specific literals (e.g. an installation id).
  const lr = createRedactor({ literals: [parseLiteralSpec('0b5e8c1e-7a4f-4c2d-9e3b-5f1a2b3c4d5e=<INSTALLATION_ID>')] });
  const lj = lr.redactJsonl('{"installationId":"0b5e8c1e-7a4f-4c2d-9e3b-5f1a2b3c4d5e"}\n');
  check('finding6: a run literal is redacted and its residual is checked', lj.text.includes('<INSTALLATION_ID>') && !lj.text.includes('0b5e8c1e') && lr.scan('0b5e8c1e-7a4f-4c2d-9e3b-5f1a2b3c4d5e').residualLeaks.length === 1);

  const frame = '{"jsonrpc":"2.0","method":"x","params":{"systemPrompt":"..."}}\n{"note":"prompt_snapshot here"}\n';
  const jl = r.redactJsonl(frame);
  check('redact: JSONL hazard in a protocol frame is reported, not silently dropped', jl.report.hazardProtocolFrames.length === 1 && jl.report.droppedHazardLines === 1 && !reportIsClean(jl.report));
}

async function unitGuards() {
  check('guard: isHerdrWait covers agent wait, pane wait-output, agent prompt --wait', isHerdrWait(['agent', 'wait', 'x']) && isHerdrWait(['pane', 'wait-output', 'p']) && isHerdrWait(['agent', 'prompt', 'x', 't', '--wait']) && !isHerdrWait(['agent', 'prompt', 'x', 't']) && !isHerdrWait(['agent', 'read', 'x']));
  const name = makeSessionName('smoke / weird name!!', new Date('2026-09-28T12:34:56.789Z'), 'abc123');
  check('guard: session name is herdr-valid', /^[A-Za-z0-9._-]{1,64}$/.test(name) && name === 'oac-k-smoke-weird-name-20260928T123456Z-abc123', name);
  const { env, delta } = herdrLaunchEnv({ PATH: '/bin', HERDR_SOCKET_PATH: '/x.sock', HERDR_SESSION: 's', herdr_log: 'debug' }, '/tmp/cfg.toml');
  check('guard: inherited HERDR_* stripped from the herdr launch env', !('HERDR_SOCKET_PATH' in env) && !('HERDR_SESSION' in env) && !('herdr_log' in env) && env.HERDR_CONFIG_PATH === '/tmp/cfg.toml' && env.PATH === '/bin' && delta.removed.length === 3);

  const base = mkdtempSync(join(tmpdir(), 'oac-herdr-unit-'));
  try {
    const commands = [];
    const s = new HerdrSession({
      herdrCmd: [process.execPath, FAKE],
      sessionName: 'unit',
      env: { ...process.env, FAKE_HERDR_STATE: join(base, 'state') },
      cwd: base,
      timebox: { remainingMs: () => 60000 },
      commands,
    });
    const rejects = async (fn, re) => {
      try {
        await fn();
        return false;
      } catch (e) {
        return e instanceof DriverError && re.test(e.message);
      }
    };
    check('guard: a wait without a timeout is refused before running', (await rejects(() => s.exec('wait', ['agent', 'wait', 'x']), /without an explicit timeout/)) && commands.length === 0);
    check('guard: unknown role refused', await rejects(() => s.exec('poke', ['agent', 'get', 'x']), /unknown herdr command role/));
    s.inputHalted = 'test';
    check('guard: input refused once halted', (await rejects(() => s.agentPrompt('x', 'again'), /halted after a timeout/)) && (await rejects(() => s.paneRun('p', 'ls'), /halted/)) && commands.length === 0);
    check('fix8: after a timeout, input mislabelled as a read is still refused', (await rejects(() => s.exec('read', ['agent', 'prompt', 'x', 'again']), /must carry an input role/)) && (await rejects(() => s.exec('wait', ['pane', 'run', 'p', 'ls']), /must carry an input role/)) && commands.length === 0);
    s.inputHalted = null;
    check('fix8: input commands are classified from argv', isInputCommand(['agent', 'prompt', 'x', 't']) && isInputCommand(['agent', 'send-keys', 'x', 'enter']) && isInputCommand(['agent', 'start', 'x']) && isInputCommand(['pane', 'run', 'p', 'ls']) && isInputCommand(['pane', 'send-text', 'p', 't']) && !isInputCommand(['agent', 'read', 'x']) && !isInputCommand(['pane', 'wait-output', 'p']));
    check('fix8: mislabelled input refused even when not halted', (await rejects(() => s.exec('read', ['agent', 'send-keys', 'x', 'enter']), /must carry an input role/)) && commands.length === 0);
    check('guard: dialog-accept refused without a preceding read', await rejects(() => s.dialogAccept('x'), /not a read/));
    check('guard: ROLES are the manifest vocabulary', ['operator-input', 'dialog-accept', 'wait', 'read'].every((r) => ROLES.includes(r)));
  } finally {
    rmSync(base, { recursive: true, force: true });
  }

  const t0 = Date.now();
  const r = await runBounded(process.execPath, ['-e', 'setInterval(() => {}, 1 << 30)'], { deadlineMs: 300 });
  check('proc: a hanging child is killed at its deadline', r.timedOut && r.killedByDriver && Date.now() - t0 < 4000, JSON.stringify({ ...r, stdout: undefined, stderr: undefined }));
  let threw = false;
  try {
    await runBounded(process.execPath, ['-v'], {});
  } catch {
    threw = true;
  }
  check('proc: runBounded refuses to run without a deadline', threw);
}

// --- lifecycle (driver end to end against the fake herdr) --------------------------------

function makeBase(stateUnder = []) {
  const base = mkdtempSync(join(tmpdir(), 'oac-herdr-selftest-'));
  const state = join(base, ...stateUnder, 'state');
  const env = { CLAUDE_CONFIG_DIR: join(base, 'claude-cfg'), CODEX_HOME: join(base, 'codex-home') };
  // Synthetic harness config for the before/after hash check. This deliberately does what
  // oac-boundaries check 10 tells driver code never to do -- hand a harness-config path held
  // in a variable to a write -- and the lint cannot see it. It is confined to test setup:
  // CLAUDE_CONFIG_DIR/CODEX_HOME are pointed into a fresh mkdtemp dir, the paths come from
  // the driver's own harnessConfigFiles() for that env, and the guard below refuses any path
  // outside that temp dir, so a real harness config can never be written. Driver code
  // (tools/herdr/lib, run.mjs) only reads and hashes these files.
  const files = {};
  for (const f of harnessConfigFiles(env)) {
    if (!within(f.path, base)) throw new Error(`selftest refused to write outside its temp dir: ${f.label}`);
    mkdirSync(dirname(f.path), { recursive: true });
    const content = `synthetic ${f.label}\n`;
    writeFileSync(f.path, content);
    files[f.label] = { path: f.path, sha256: sha(content), content };
  }
  return { base, state, env, files };
}

function driverEnv(b, mode) {
  return {
    ...process.env,
    ...b.env,
    FAKE_HERDR_STATE: b.state,
    FAKE_HERDR_MODE: mode ?? '',
    CODEX_THREAD_ID: 'selftest-thread',
    HERDR_SOCKET_PATH: join(b.base, 'enclosing', 'herdr.sock'),
    HERDR_SESSION: 'enclosing-session',
  };
}

function collect(b, res) {
  const read = (p) => (existsSync(p) ? readFileSync(p, 'utf8') : '');
  const lines = (p) => read(p).split('\n').filter(Boolean).map((l) => JSON.parse(l));
  const manifestPath = join(b.base, 'out', 'run-manifest.json');
  return {
    ...res,
    manifestText: read(manifestPath),
    manifest: existsSync(manifestPath) ? JSON.parse(read(manifestPath)) : null,
    calls: lines(join(b.state, 'calls.log')),
    prompts: lines(join(b.state, 'prompts.log')),
    capture: (name) => read(join(b.base, 'out', name)),
  };
}

function runDriver({ scenario = 'smoke', mode, args = [], herdrBin = FAKE, stateUnder }) {
  const b = makeBase(stateUnder);
  const res = spawnSync(process.execPath, [RUN, '--scenario', scenario, '--herdr-bin', herdrBin, '--out', join(b.base, 'out'), ...args], {
    env: driverEnv(b, mode),
    encoding: 'utf8',
    timeout: 90000,
  });
  return { b, r: collect(b, { status: res.status, stdout: res.stdout, stderr: res.stderr }) };
}

// Invariants every lifecycle run must hold, whatever its outcome.
function invariants(name, b, r) {
  const m = r.manifest;
  check(`${name}: run-manifest.json written`, !!m, r.stdout + r.stderr);
  if (!m) return;
  const commands = m.commands;
  check(`${name}: every command carries a known role`, commands.every((c) => ROLES.includes(c.role)));
  check(`${name}: every herdr wait carries --timeout`, commands.filter((c) => isHerdrWait(c.argv.slice(3))).every((c) => c.argv.includes('--timeout')));
  check(`${name}: every command had a driver deadline or is the supervised server`, commands.every((c) => c.bound.driverDeadlineMs > 0 || c.argv.at(-1) === 'server'));
  const sessionCalls = r.calls.filter((c) => c.argv[0] !== '--version' && c.argv[0] !== 'session');
  check(`${name}: every session-scoped herdr call names the run's session`, sessionCalls.every((c) => c.argv[0] === '--session' && c.argv[1] === m.session.name));
  check(`${name}: no herdr call inherited an enclosing HERDR_SOCKET_PATH`, r.calls.every((c) => c.inheritedSocketPath === null));
  check(`${name}: every herdr call ran with the run's own herdr config`, r.calls.every((c) => typeof c.configPath === 'string' && c.configPath.endsWith('herdr-config.toml')));
  check(`${name}: the harness-integration command family was never called`, r.calls.every((c) => !c.argv.includes('integration')));
  check(`${name}: timebox start and end recorded`, !!m.timebox.start && !!m.timebox.end && m.timebox.elapsedMs >= 0);
  check(`${name}: driver commit and OS recorded`, /^[0-9a-f]{40}$/.test(m.driver.commit ?? '') && !!m.os.platform);
  check(`${name}: harness config hashed before and after, unchanged`, m.harnessConfig.before.length === 3 && m.harnessConfig.before.every((h) => h.present && h.sha256 === b.files[h.file]?.sha256) && m.harnessConfig.unchanged === true, JSON.stringify({ manifest: m.harnessConfig.before, expected: b.files }));
  check(`${name}: synthetic harness config left byte-identical`, Object.values(b.files).every((f) => readFileSync(f.path, 'utf8') === f.content));
  check(`${name}: no raw scratch or home path in the manifest`, !r.manifestText.includes('oac-herdr-scratch-') && !(homedir().length > 1 && r.manifestText.includes(homedir())));
  const pids = [m.session.serverPid, ...m.session.panePids].filter(Boolean);
  check(`${name}: no server or pane process left running`, pids.every((p) => !isAlive(p)), JSON.stringify(pids.filter(isAlive)));
  const scan = processesForSession(m.session.name);
  check(`${name}: no process still carries --session <name>`, scan === null || scan.length === 0);
  if (m.session.serverPid) {
    check(`${name}: session stopped and deleted`, r.calls.some((c) => c.argv[0] === 'session' && c.argv[1] === 'stop') && r.calls.some((c) => c.argv[0] === 'session' && c.argv[1] === 'delete') && !existsSync(join(b.state, 'sessions', m.session.name)));
  }
  check(`${name}: scratch removed`, m.scratch.removed === true);
  check(`${name}: written manifest scans clean of residual hits`, m.manifestRedaction?.writtenClean === true);
}

function afterTimeoutNoInput(m) {
  const idx = m.commands.findIndex((c) => c.timedOut);
  return idx !== -1 && m.commands.slice(idx + 1).every((c) => !['operator-input', 'dialog-accept'].includes(c.role));
}

async function lifecycle() {
  const scratchBefore = new Set(readdirSync(tmpdir()).filter((n) => n.startsWith('oac-herdr-scratch-')));
  const cases = [];
  const run = (name, opts, assert) => cases.push({ name, opts, assert });

  run('smoke PASS', {}, (r) => {
    const m = r.manifest;
    check('smoke PASS: exit 0, outcome PASS', r.status === 0 && m.outcome === 'PASS', `${r.status} ${m.outcome} ${m.outcomeReason}`);
    check('smoke PASS: herdr version checked against the PINS.md pin', m.herdr.observedVersionOutput === m.herdr.expectedVersionOutput && /^v\d/.test(m.herdr.pinnedTag));
    check('smoke PASS: herdr run config disables version and manifest checks', /version_check = false/.test(m.herdr.config.contents) && /manifest_check = false/.test(m.herdr.config.contents) && m.herdr.config.path === '<SCRATCH>/herdr-config.toml');
    const roles = new Set(m.commands.map((c) => c.role));
    check('smoke PASS: roles preflight, lifecycle, read, wait, operator-input all recorded', ['preflight', 'lifecycle', 'read', 'wait', 'operator-input'].every((x) => roles.has(x)), [...roles].join(','));
    check('smoke PASS: launch argv recorded verbatim', JSON.stringify(m.launch.argv) === JSON.stringify(['echo', 'OAC-SMOKE-READY']));
    check('smoke PASS: server launch env delta recorded', m.env.serverLaunch.added.HERDR_CONFIG_PATH === '<SCRATCH>/herdr-config.toml' && m.env.serverLaunch.removed.includes('HERDR_SOCKET_PATH') && m.env.serverLaunch.removed.includes('HERDR_SESSION'));
    const pane = m.env.pane;
    check('smoke PASS: pane env delta has the HERDR_* additions', ['HERDR_ENV', 'HERDR_SOCKET_PATH', 'HERDR_BIN_PATH', 'HERDR_WORKSPACE_ID', 'HERDR_TAB_ID', 'HERDR_PANE_ID'].every((n) => pane?.added.includes(n)), JSON.stringify(pane?.added));
    check('smoke PASS: pane env delta has the removals', pane?.removed.includes('CODEX_THREAD_ID'), JSON.stringify(pane?.removed));
    check('smoke PASS: pane env values limited to HERDR_*, TERM, COLORTERM', Object.keys(pane?.values ?? {}).every((k) => /^HERDR_/.test(k) || k === 'TERM' || k === 'COLORTERM') && pane.values.HERDR_ENV === '1');
    check('smoke PASS: harness versions N/A (no harness launched)', /N\/A/.test(m.harnessVersions.note ?? ''));
    const cap = r.capture('pane-smoke.txt');
    check('smoke PASS: pane capture written, redacted, shows the output line', cap.split('\n').some((l) => l.trim() === 'OAC-SMOKE-READY') && !cap.includes('oac-herdr-scratch-') && cap.includes('<SCRATCH>'));
    check('smoke PASS: capture redaction report is clean', m.captures[0]?.written === true && m.captures[0].redaction.residualLeaks.length === 0 && m.captures[0].redaction.residualGenericHits.length === 0);
    check('smoke PASS: teardown clean', m.teardown.clean === true, JSON.stringify(m.teardown));
  });
  run('custom launch', { args: ['--launch', '["echo","CUSTOM-LAUNCH-42"]', '--param', 'expect=CUSTOM-LAUNCH-42'] }, (r) => {
    check('custom launch: PASS with the parameterized command', r.status === 0 && r.manifest.outcome === 'PASS', r.manifest?.outcomeReason);
    check('custom launch: launch argv verbatim and typed into the pane', JSON.stringify(r.manifest.launch.argv) === '["echo","CUSTOM-LAUNCH-42"]' && r.calls.some((c) => c.argv.includes('run') && c.argv.at(-1) === 'echo CUSTOM-LAUNCH-42'));
  });
  run('version mismatch', { mode: 'version=0.9.0' }, (r) => {
    check('version mismatch: NOT RUN (exit 3), refused', r.status === 3 && r.manifest.outcome === 'NOT RUN' && /Refusing to run/.test(r.manifest.outcomeReason));
    check('version mismatch: no server started', r.calls.length === 1 && r.calls[0].argv[0] === '--version' && r.manifest.session.serverPid === null);
  });
  run('preview build', { mode: 'version=0.9.1-preview.abc' }, (r) => {
    check('preview build: NOT RUN', r.status === 3 && r.manifest.outcome === 'NOT RUN');
  });
  run('server version mismatch', { mode: 'server-version=0.9.0' }, (r) => {
    check('server version mismatch: NOT RUN, torn down', r.status === 3 && /server reports version/.test(r.manifest.outcomeReason) && r.manifest.teardown.clean);
  });
  run('herdr missing', { herdrBin: '/nonexistent/oac-selftest/herdr' }, (r) => {
    check('herdr missing: NOT RUN', r.status === 3 && r.manifest.outcome === 'NOT RUN' && /could not be checked/.test(r.manifest.outcomeReason));
  });
  run('herdr wait timeout', { mode: 'never-match', args: ['--param', 'waitMs=800'] }, (r) => {
    const m = r.manifest;
    const t = m.commands.find((c) => c.timedOut);
    check('herdr wait timeout: NOT RUN (exit 3), not FAIL', r.status === 3 && m.outcome === 'NOT RUN', `${r.status} ${m.outcome}`);
    check('herdr wait timeout: herdr reported the timeout', t?.timeoutBy === 'herdr' && t.errorCode === 'timeout' && t.role === 'wait');
    check('herdr wait timeout: no input after the timeout', afterTimeoutNoInput(m));
    check('herdr wait timeout: teardown clean', m.teardown.clean);
  });
  run('driver deadline on a wait', { mode: 'hang-wait', args: ['--param', 'waitMs=800'] }, (r) => {
    const t = r.manifest.commands.find((c) => c.timedOut);
    check('driver deadline on a wait: NOT RUN via driver kill', r.status === 3 && t?.timeoutBy === 'driver' && t.killedByDriver && t.durationMs < 800 + 5000 + 3000, JSON.stringify(t));
    check('driver deadline on a wait: teardown clean', r.manifest.teardown.clean);
  });
  run('agent read hang', { scenario: join(HERE, 'scenarios', 'agent-io.mjs'), mode: 'hang-agent-read', args: ['--param', 'op=read'] }, (r) => {
    const t = r.manifest.commands.find((c) => c.timedOut);
    check('agent read hang: NOT RUN via driver deadline', r.status === 3 && t?.argv.includes('read') && t.timeoutBy === 'driver');
    check('agent read hang: no --timeout passed to agent read (herdr has none)', !t.argv.includes('--timeout') && t.bound.herdrTimeoutMs === null && /driver deadline only/.test(t.bound.by));
  });
  run('send-keys hang', { scenario: join(HERE, 'scenarios', 'agent-io.mjs'), mode: 'hang-send-keys', args: ['--param', 'op=send-keys'] }, (r) => {
    const t = r.manifest.commands.find((c) => c.timedOut);
    check('send-keys hang: NOT RUN via driver deadline', r.status === 3 && t?.argv.includes('send-keys') && t.timeoutBy === 'driver' && !t.argv.includes('--timeout'));
  });
  run('prompt timeout never re-submits', { scenario: join(HERE, 'scenarios', 'agent-io.mjs'), mode: 'prompt-timeout', args: ['--param', 'op=prompt-retry'] }, (r) => {
    check('prompt timeout: NOT RUN', r.status === 3 && r.manifest.outcome === 'NOT RUN');
    check('prompt timeout: exactly one prompt reached herdr', r.prompts.length === 1, String(r.prompts.length));
    check('prompt timeout: the retry was refused by the driver', /^refused: .*halted after a timeout/.test(r.manifest.scenarioData.retry ?? ''), r.manifest.scenarioData.retry);
  });
  run('dialog accept without read', { scenario: join(HERE, 'scenarios', 'agent-io.mjs'), args: ['--param', 'op=dialog-no-read'] }, (r) => {
    check('dialog accept without read: FAIL, refused', r.status === 1 && /not a read/.test(r.manifest.outcomeReason) && !r.manifest.commands.some((c) => c.role === 'dialog-accept'));
  });
  run('dialog accept after read', {
    scenario: join(HERE, 'scenarios', 'agent-io.mjs'),
    args: ['--param', 'op=dialog-after-read', '--launch', '["claude","--dangerously-load-development-channels","server:selftest"]'],
  }, (r) => {
    const m = r.manifest;
    check('dialog accept after read: PASS with a dialog-accept command', r.status === 0 && m.commands.some((c) => c.role === 'dialog-accept'), m.outcomeReason);
    check('dialog accept after read: launch argv verbatim, mapped to --kind and passthrough args', JSON.stringify(m.launch.argv) === JSON.stringify(m.launch.herdrReportedArgv) && r.calls.some((c) => c.argv.join(' ').includes('agent start selftest --kind claude --pane w1:p1 --timeout 10000 -- --dangerously-load-development-channels server:selftest')));
  });
  run('scenario throws', { scenario: join(HERE, 'scenarios', 'throws.mjs') }, (r) => {
    check('scenario throws: FAIL (exit 1) with the error, torn down', r.status === 1 && /planted scenario failure/.test(r.manifest.outcomeReason) && r.manifest.teardown.clean);
  });
  run('unbounded wait', { scenario: join(HERE, 'scenarios', 'unbounded-wait.mjs') }, (r) => {
    check('unbounded wait: FAIL before the wait reaches herdr', r.status === 1 && /without an explicit timeout/.test(r.manifest.outcomeReason) && !r.calls.some((c) => c.argv.includes('wait-output')));
  });
  run('server ignores stop', { mode: 'server-ignores-stop' }, (r) => {
    const m = r.manifest;
    check('server ignores stop: FAIL (teardown needed force)', r.status === 1 && m.outcome === 'FAIL' && m.teardown.forcedKills.some((k) => k.what === 'herdr server') && m.teardown.leftoverProcesses.length === 0, JSON.stringify(m.teardown));
  });
  run('pane leaked', { mode: 'leak-pane' }, (r) => {
    const m = r.manifest;
    check('pane leaked: FAIL, pane process killed and recorded', r.status === 1 && m.teardown.forcedKills.some((k) => /pane process/.test(k.what)) && m.teardown.leftoverProcesses.length === 0, JSON.stringify(m.teardown));
  });
  run('residual withheld', { stateUnder: ['scratchpad', '0b5e8c1e-7a4f-4c2d-9e3b-5f1a2b3c4d5e'] }, (r) => {
    const m = r.manifest;
    const w = m.manifestRedaction.withheld;
    check('residual withheld: a value redaction could not clear is withheld, not written', w.some((x) => x.path === '$.env.pane.values.HERDR_SOCKET_PATH') && m.env.pane.values.HERDR_SOCKET_PATH.startsWith('<WITHHELD') && !r.manifestText.includes('0b5e8c1e-7a4f'), JSON.stringify(w));
    check('residual withheld: written manifest scans clean, run still PASS', m.manifestRedaction.writtenClean === true && m.outcome === 'PASS' && m.findings.some((f) => /withheld/.test(f)), m.outcomeReason);
  });
  const T = (f) => join(HERE, 'scenarios', f);
  run('fix1: scenario catches a timeout and returns', { scenario: T('catches-timeout.mjs'), mode: 'prompt-timeout' }, (r) => {
    const m = r.manifest;
    check('fix1: caught timeout still ends NOT RUN (exit 3), never PASS', r.status === 3 && m.outcome === 'NOT RUN' && /scenario returned normally, but command #\d+ \(operator-input\) timed out/.test(m.outcomeReason) && m.commands.some((c) => c.timedOut) && !!m.scenarioData.caught, `${r.status} ${m.outcome} ${m.outcomeReason}`);
  });
  run('fix1: scenario catches a timeout then fails', { scenario: T('catches-timeout.mjs'), mode: 'prompt-timeout', args: ['--param', 'then=throw'] }, (r) => {
    const m = r.manifest;
    check('fix1: failure after a caught timeout is NOT RUN, not FAIL', r.status === 3 && m.outcome === 'NOT RUN' && m.findings.some((f) => /recorded as NOT RUN/.test(f)), `${r.status} ${m.outcome}`);
  });
  run('fix1: scenario outlives its timebox without a herdr call', { scenario: T('slow.mjs'), args: ['--timebox-ms', '1000', '--param', 'sleepMs=1600'] }, (r) => {
    const m = r.manifest;
    check('fix1: expired timebox ends NOT RUN even though the scenario returned', r.status === 3 && /the timebox expired/.test(m.outcomeReason) && m.timebox.expired === true, `${r.status} ${m.outcomeReason}`);
  });
  run('fix7: non-herdr await past the timebox', { scenario: T('slow.mjs'), args: ['--timebox-ms', '1000', '--param', 'sleepMs=120000'] }, (r) => {
    const m = r.manifest;
    const wall = Date.parse(m.timebox.teardownEnd) - Date.parse(m.timebox.start);
    check('fix7: timebox enforced while the scenario awaits something other than herdr', r.status === 3 && /timebox expired/.test(m.outcomeReason) && wall < 20000 && m.teardown.clean, `${r.status} ${m.outcomeReason} wall=${wall}`);
  });
  run('fix8: a failed read does not unlock dialog-accept', { scenario: T('agent-io.mjs'), mode: 'fail-agent-read', args: ['--param', 'op=dialog-after-failed-read'] }, (r) => {
    const m = r.manifest;
    check('fix8: dialog-accept refused after a failed read', r.status === 1 && /not a read/.test(m.outcomeReason) && !m.commands.some((c) => c.role === 'dialog-accept'), `${r.status} ${m.outcomeReason}`);
  });
  run('fix2: pane capture with a private key', { scenario: T('capture.mjs'), args: ['--param', 'kind=key'] }, (r) => {
    const cap = r.capture('key.txt');
    check('fix2: written capture has no key body or END line, reports clean', cap === 'before the key\nafter the key' && r.manifest.captures[0].written && r.manifest.captures[0].redaction.droppedHazardLines === 5 && r.manifest.outcome === 'PASS', JSON.stringify(cap));
  });
  run('fix3: captures with JSON-shaped headers', { scenario: T('capture.mjs'), args: ['--param', 'kind=headers'] }, (r) => {
    const txt = r.capture('headers.txt');
    const jl = r.capture('headers.jsonl');
    check('fix3: text capture drops JSON-shaped header lines', txt === 'clean line', JSON.stringify(txt));
    check('fix3: JSONL capture redacts header values by key', jl.includes('"Authorization":"<SECRET>"') && jl.includes('"Cookie":"<SECRET>"') && !/c2VjcmV0|abc123def456/.test(jl), jl);
  });
  run('finding11: hazard in a JSONL protocol frame', { scenario: T('capture.mjs'), args: ['--param', 'kind=frame'] }, (r) => {
    const m = r.manifest;
    check('finding11: protocol-frame hazard fails closed: capture withheld, run FAIL', r.status === 1 && m.captures[0].written === false && m.captures[0].redaction.hazardProtocolFrames.length === 1 && r.capture('frame.jsonl') === '', `${r.status} ${m.outcomeReason}`);
  });
  run('finding6: run literal via ctx.redactLiteral', { scenario: T('capture.mjs'), args: ['--param', 'kind=literal'] }, (r) => {
    const m = r.manifest;
    check('finding6: literal redacted in capture and manifest; only the placeholder recorded', r.capture('literal.jsonl').includes('<INSTALLATION_ID>') && !r.capture('literal.jsonl').includes('0b5e8c1e') && !r.manifestText.includes('0b5e8c1e') && m.scenarioData.installationId === '<INSTALLATION_ID>' && m.redaction.runLiteralPlaceholders.includes('<INSTALLATION_ID>'));
  });
  run('finding6: run literal via --redact-literal', { args: ['--redact-literal', 'OAC-SMOKE-READY=<SMOKE_MARKER>'] }, (r) => {
    check('finding6: CLI literal redacted in the capture', r.capture('pane-smoke.txt').includes('<SMOKE_MARKER>') && !r.capture('pane-smoke.txt').includes('OAC-SMOKE-READY') && !r.manifestText.includes('OAC-SMOKE-READY'));
  });
  run('finding9: harness child process outlives its pane', { mode: 'pane-child-survives' }, (r) => {
    const m = r.manifest;
    check('finding9: a pane descendant left running is found, killed, and fails the run', r.status === 1 && m.teardown.forcedKills.some((k) => /descendant/.test(k.what)) && m.teardown.leftoverProcesses.length === 0 && m.session.panePids.length >= 2, JSON.stringify(m.teardown));
  });
  run('timebox', { mode: 'never-match', args: ['--timebox-ms', '2500', '--param', 'waitMs=60000'] }, (r) => {
    const t = r.manifest.commands.find((c) => c.timedOut);
    check('timebox: wait clipped to the timebox, run NOT RUN', r.status === 3 && t?.bound.clippedToTimebox === true && t.bound.herdrTimeoutMs < 2500, JSON.stringify(t?.bound));
  });

  for (const c of cases) {
    const { b, r } = runDriver(c.opts);
    try {
      invariants(c.name, b, r);
      if (r.manifest) c.assert(r);
    } catch (err) {
      check(`${c.name}: assertions ran`, false, err.stack);
    } finally {
      rmSync(b.base, { recursive: true, force: true });
    }
  }

  // Operator abort: SIGINT mid-wait ends NOT RUN and still tears down.
  {
    const b = makeBase();
    const child = spawn(process.execPath, [RUN, '--scenario', 'smoke', '--herdr-bin', FAKE, '--out', join(b.base, 'out'), '--param', 'waitMs=60000'], { env: driverEnv(b, 'hang-wait'), stdio: ['ignore', 'pipe', 'pipe'] });
    let stdout = '';
    child.stdout.on('data', (d) => (stdout += d));
    const calls = join(b.state, 'calls.log');
    const t0 = Date.now();
    while (Date.now() - t0 < 20000 && !(existsSync(calls) && readFileSync(calls, 'utf8').split('\n').filter((l) => l.includes('wait-output')).length >= 2)) {
      await new Promise((res) => setTimeout(res, 100));
    }
    child.kill('SIGINT');
    const status = await new Promise((res) => child.on('close', res));
    const r = collect(b, { status, stdout, stderr: '' });
    try {
      invariants('SIGINT', b, r);
      check('SIGINT: NOT RUN (operator abort), teardown clean', status === 3 && /operator abort/.test(r.manifest?.outcomeReason ?? '') && r.manifest.teardown.clean, `${status} ${r.manifest?.outcomeReason}`);
    } finally {
      rmSync(b.base, { recursive: true, force: true });
    }
  }

  // Fix 7: SIGINT (twice) while the scenario awaits a non-herdr operation: the first signal
  // settles the run, the second only warns; teardown and the manifest still happen.
  {
    const b = makeBase();
    const t0 = Date.now();
    const child = spawn(process.execPath, [RUN, '--scenario', join(HERE, 'scenarios', 'slow.mjs'), '--herdr-bin', FAKE, '--out', join(b.base, 'out'), '--param', 'sleepMs=120000'], { env: driverEnv(b, ''), stdio: ['ignore', 'pipe', 'pipe'] });
    let stdout = '';
    let stderr = '';
    child.stdout.on('data', (d) => (stdout += d));
    child.stderr.on('data', (d) => (stderr += d));
    const calls = join(b.state, 'calls.log');
    while (Date.now() - t0 < 20000 && !(existsSync(calls) && readFileSync(calls, 'utf8').includes('process-info'))) {
      await new Promise((res) => setTimeout(res, 100));
    }
    child.kill('SIGINT');
    await new Promise((res) => setTimeout(res, 50));
    child.kill('SIGINT');
    const status = await new Promise((res) => child.on('close', res));
    const r = collect(b, { status, stdout, stderr });
    try {
      invariants('SIGINT x2 during non-herdr await', b, r);
      check('fix7: SIGINT during a non-herdr await ends NOT RUN promptly, manifest written, teardown clean', status === 3 && /operator abort/.test(r.manifest?.outcomeReason ?? '') && r.manifest.teardown.clean && Date.now() - t0 < 30000, `${status} ${r.manifest?.outcomeReason} ${Date.now() - t0}ms`);
      check('fix7: the second SIGINT only warns (no exit before teardown)', /teardown in progress/.test(stderr) || status === 3);
    } finally {
      rmSync(b.base, { recursive: true, force: true });
    }
  }

  const leftover = readdirSync(tmpdir()).filter((n) => n.startsWith('oac-herdr-scratch-') && !scratchBefore.has(n));
  check('lifecycle: every run removed its scratch directory', leftover.length === 0, leftover.join(','));
}

export async function runSelfTest() {
  console.log('herdr driver self-test (unit)');
  unitPins();
  unitQuoting();
  unitRedaction();
  await unitGuards();
  if (process.platform === 'win32') {
    console.log('lifecycle checks skipped: the fake herdr runs pane commands with sh (POSIX only)');
  } else {
    console.log('herdr driver self-test (lifecycle, against test/fake-herdr.mjs -- not herdr)');
    await lifecycle();
  }
  console.log(`\nself-test: ${passed}/${passed + failed} checks passed${failed ? `, ${failed} FAILED` : ''}.`);
  return failed ? 1 : 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = await runSelfTest();
}
