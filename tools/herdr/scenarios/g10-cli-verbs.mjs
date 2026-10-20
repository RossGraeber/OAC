// g10-cli-verbs: G10 (#71) slice 6, the live check of the CLI verbs. NOT VERDICT-BEARING (G10 is not a gate).
//
// Drives the REAL `oac` binary, with no config file and no test namespace, through:
//   oac status (no daemon, exit 3) -> oac start --detach -> oac status -> a real Claude Code session
//   (print mode) that appears in oac sessions -> oac doctor -> that session ends -> oac stop ->
//   oac status and oac sessions (exit 3); then reads the daemon's own log file for the line
//   "shut down: withdrawals sent".
//
// The real `oac start` uses the user's REAL device key in the operating system's credential store
// (Windows: target `device-key.io.github.rossgraeber.oac`, made in G9 leg C with the lead's approval).
// This scenario never reads or prints that secret and never creates a second key on Windows. On a host
// with no credential service (WSL) `oac start` would make a NEW key (the encrypted-file fallback, which
// needs OAC_DEVICE_KEY_PASSPHRASE): do not run it there without the lead's approval of that key.
//
// Print mode shows no dialog. The MCP server and the hook come from --mcp-config / --strict-mcp-config
// and --settings files in scratch, so no project file is written, no trust or MCP-approval dialog
// appears, nothing is signed in and no credential file is read. The session is kept alive for
// `holdSeconds` by one allowed Bash call (`sleep N`, rule `Bash(sleep *)`, documented at
// https://code.claude.com/docs/en/permissions, retrieved 2026-10-20), so `oac sessions` can see it.
//
// The scenario refuses to run if a daemon is already running (it never stops one it did not start).
// Pane text is redacted by the driver (a Windows account SID becomes <SID>, #431); the session id seen
// in `oac sessions` is also redacted as <SESSION_ID>.
//
// Operator command:
//   node tools/herdr/run.mjs --scenario g10-cli-verbs --param oac=<abs path to oac(.exe)> \
//     [--param holdSeconds=45] [--param claudeWaitMs=240000] --out <run dir>

import { readFileSync, writeFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import { randomBytes } from 'node:crypto';

import { regexLiteral } from '../lib/pane-shell.mjs';

function jsonObjects(text) {
  const out = [];
  for (const line of text.split(/\r?\n/)) {
    const t = line.trim();
    if (!t.startsWith('{')) continue;
    try {
      const o = JSON.parse(t);
      if (o && o.oac_cli === 1) out.push(o);
    } catch {
      /* a partial line */
    }
  }
  return out;
}

export default {
  name: 'g10-cli-verbs',
  description: 'Real oac start --detach, status, sessions (a real Claude Code session), doctor, stop; presence withdrawn.',
  harnesses: ['claude'],
  defaults: {
    launch: [],
    timeboxMs: 900000,
    params: { oac: '', holdSeconds: '45', waitMs: '60000', claudeWaitMs: '240000', pollAttempts: '20' },
  },
  async run(ctx) {
    const hooks = { stopIfStarted: null };
    try {
      await runInner(ctx, hooks);
    } catch (err) {
      // A daemon this run started is stopped cleanly (oac stop), never killed.
      if (hooks.stopIfStarted) {
        try {
          await hooks.stopIfStarted();
        } catch {
          ctx.finding('the cleanup oac stop failed: a detached daemon may be left running');
        }
      }
      throw err;
    }
  },
};

async function runInner(ctx, hooks) {
  {
    const { herdr, params } = ctx;
    if (!params.oac) throw new Error('--param oac=<abs path to oac(.exe)> is required');
    if (process.env.OAC_SCOPE) throw new Error('OAC_SCOPE is set: oac start refuses a test namespace, and this scenario uses the real scope');
    const waitMs = Number(params.waitMs);
    const claudeWaitMs = Number(params.claudeWaitMs);
    const hold = Number(params.holdSeconds);
    const attempts = Number(params.pollAttempts);
    if (!Number.isInteger(hold) || hold < 10 || hold > 300) throw new Error('holdSeconds must be an integer from 10 to 300');
    const work = ctx.dir('g10');
    const slash = (p) => p.replace(/\\/g, '/');
    const exitVar = ctx.paneShell === 'powershell' ? '$LASTEXITCODE' : '$?';
    const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

    const mcpPath = join(work, 'mcp.json');
    const settingsPath = join(work, 'settings.json');
    // No OAC_SCOPE anywhere: the real default scope.
    writeFileSync(mcpPath, JSON.stringify({ mcpServers: { oac: { command: params.oac, args: ['mcp-shim', '--harness', 'claude'] } } }));
    writeFileSync(
      settingsPath,
      JSON.stringify({ hooks: { SessionStart: [{ hooks: [{ type: 'command', command: `"${slash(params.oac)}" hook` }] }] } }),
    );
    ctx.record('g10', { nonVerdictBearing: 'G10 #71 slice 6: not a gate; records what the verbs reported, and never changes a gate verdict' });
    ctx.record('deviceKey', 'the real key was used by oac start; never read or printed by this scenario');

    const wsA = await herdr.workspaceCreate({ cwd: work, label: 'oac-g10-ops', env: {} });
    ctx.record('opsPane', wsA);

    // Runs one verb in the ops pane and returns { code, text, objects } for what it printed.
    let n = 0;
    async function step(tag, argv) {
      n += 1;
      const id = `${tag}${n}-${randomBytes(3).toString('hex')}`;
      const cmd = ctx.quoteForPane([params.oac, ...argv]);
      await herdr.paneRun(wsA.paneId, `echo G10-${id}-BEGIN; ${cmd}; echo G10-${id}-EXIT-${exitVar}`);
      await herdr.paneWaitOutput(wsA.paneId, { regex: `^\\s*G10-${regexLiteral(id)}-EXIT-\\d+\\s*$`, timeoutMs: waitMs });
      const all = await herdr.paneRead(wsA.paneId, { source: 'recent-unwrapped', lines: 400 });
      const lines = all.split(/\r?\n/);
      const b = lines.findIndex((l) => new RegExp(`^\\s*G10-${regexLiteral(id)}-BEGIN\\s*$`).test(l));
      const e = lines.findIndex((l) => new RegExp(`^\\s*G10-${regexLiteral(id)}-EXIT-\\d+\\s*$`).test(l));
      if (b < 0 || e < b) throw new Error(`step ${id}: markers not found in the pane text`);
      const text = lines.slice(b + 1, e).join('\n');
      const code = Number(lines[e].trim().split('-').pop());
      ctx.capture(`step-${n}-${tag}.txt`, `$ oac ${argv.join(' ')}\n${text}\nexit ${code}\n`);
      return { code, text, objects: jsonObjects(text) };
    }
    const last = (r) => r.objects.at(-1) ?? null;

    // 0. Precondition: no daemon is running.
    const pre = await step('pre-status', ['status', '--json']);
    ctx.record('preStatus', { exit: pre.code, error: last(pre)?.error ?? null });
    if (pre.code !== 3) {
      ctx.finding(`a daemon is already running (or oac status exited ${pre.code}); this scenario never stops one it did not start`);
      throw new Error('precondition: oac status did not exit 3');
    }

    // 1. Start detached.
    hooks.stopIfStarted = () => step('cleanup-stop', ['stop']);
    const start = await step('start', ['start', '--detach']);
    // 2. Status.
    const st = await step('status', ['status', '--json']);
    const stObj = last(st);
    const logPath = stObj?.log ?? null;
    if (stObj?.device_id) ctx.redactLiteral(stObj.device_id, '<DEVICE_ID>');
    // 3. A real Claude Code session, kept alive for `hold` seconds.
    const wsB = await herdr.workspaceCreate({ cwd: ctx.dir('claude-cwd'), label: 'oac-g10-claude', env: {} });
    ctx.record('claudePane', wsB);
    await ctx.probeEnv(wsB.paneId, { timeoutMs: waitMs });
    const mark = `G10-CLAUDE-DONE-${randomBytes(4).toString('hex')}`;
    // No double quote in the prompt: Windows PowerShell 5.1 drops an embedded `"` when it passes a
    // single-quoted argument to a native program (quoteArg in lib/pane-shell.mjs, line 13, wraps it in
    // single quotes only), which split the first run's prompt at `"sleep 45"` (STATUS, slice 6).
    const prompt =
      `Call the whoami tool of the oac MCP server once. Then run the Bash command sleep ${hold} (exactly that command). ` +
      'Then print the whoami result text exactly, nothing else.';
    if (ctx.paneShell === 'powershell' && prompt.includes('"')) throw new Error('the prompt must not contain a double quote under the PowerShell pane shell');
    const claude = ctx.quoteForPane([
      'claude', '-p', prompt,
      '--allowedTools', 'mcp__oac__whoami', 'Bash(sleep *)',
      '--mcp-config', mcpPath, '--strict-mcp-config', '--settings', settingsPath,
      '--max-turns', '6',
    ]);
    await herdr.paneRun(wsB.paneId, `${claude}; echo ${mark}`);

    // 4. Poll `oac sessions` (bounded) until the session is listed.
    let during = null;
    for (let i = 0; i < attempts && !during; i += 1) {
      await sleep(3000);
      const r = await step('sessions-during', ['sessions', '--json']);
      const o = last(r);
      if (r.code === 0 && o && o.sessions_total >= 1) during = { r, o };
    }
    let sessionId = null;
    if (during) {
      sessionId = during.o.sessions?.[0]?.session_id ?? null;
      if (sessionId) ctx.redactLiteral(sessionId, '<SESSION_ID>');
    } else {
      ctx.finding(`no session appeared in oac sessions within ${attempts} polls`);
    }
    const stDuring = await step('status-during', ['status', '--json']);
    const doctor = await step('doctor', ['doctor', '--json']);
    const doctorObj = last(doctor);

    // 5. The Claude session ends.
    try {
      await herdr.paneWaitOutput(wsB.paneId, { regex: `^\\s*${regexLiteral(mark)}\\s*$`, timeoutMs: claudeWaitMs });
    } catch (err) {
      ctx.capture('pane-claude-not-done.txt', await herdr.paneRead(wsB.paneId, { source: 'recent-unwrapped', lines: 200 }));
      throw err;
    }
    const claudeText = await herdr.paneRead(wsB.paneId, { source: 'recent-unwrapped', lines: 200 });
    ctx.capture('pane-claude.txt', claudeText);
    // A failed hold is a finding, never a silent pass: the model reported a failed sleep.
    const sleepFailed = /missing operand/i.test(claudeText);
    if (sleepFailed) ctx.finding('the held Bash call failed ("missing operand" in the Claude pane): the session was not held');
    await herdr.paneProcessInfo(wsB.paneId);
    await sleep(2000);
    const after = await step('sessions-after', ['sessions', '--json']);

    // 6. Stop, then both verbs report not running.
    const stop = await step('stop', ['stop', '--json']);
    hooks.stopIfStarted = null;
    const sAfter = await step('status-after', ['status', '--json']);
    const sessAfter = await step('sessions-final', ['sessions', '--json']);

    // 7. The daemon's own log (no credential; its contract holds no message content or key).
    let withdrawn = false;
    let logLines = 0;
    let holdGapMs = null;
    if (logPath && existsSync(logPath)) {
      const log = readFileSync(logPath, 'utf8');
      // This run's lines only: after the last `serving` line. The first `freed a connection on claude` is
      // the hook carrier's, the last the shim's; their gap is at least the hold when the hold worked.
      const mine = log.split(/\r?\n/).filter(Boolean);
      const from = mine.map((l) => l.includes('oac-daemon: serving')).lastIndexOf(true);
      // Both checks read this run's lines only: the file also holds earlier runs'.
      const thisRun = mine.slice(from + 1);
      withdrawn = thisRun.some((l) => l.includes('shut down: withdrawals sent'));
      const freed = thisRun.filter((l) => l.includes('freed a connection on claude')).map((l) => Number(/^unix_ms=(\d+) /.exec(l)?.[1]));
      if (freed.length >= 2 && freed.every(Number.isFinite)) holdGapMs = freed.at(-1) - freed[0];
      if (holdGapMs === null || holdGapMs < hold * 1000) ctx.finding(`the session was not held for ${hold} s: gap between the hook and the shim ending ${holdGapMs === null ? 'unknown' : `${holdGapMs} ms`}`);
      const tail = log.split(/\r?\n/).filter(Boolean).slice(-40);
      logLines = tail.length;
      ctx.capture('daemon-log-tail.txt', tail.join('\n') + '\n');
    }

    const checks = Object.fromEntries((doctorObj?.checks ?? []).map((c) => [c.name, c.result]));
    const observed = {
      startExit: start.code,
      statusExit: st.code,
      statusRunning: stObj?.running === true,
      sessionSeen: Boolean(during),
      sessionHarness: during?.o.sessions?.[0]?.harness ?? null,
      sessionState: during?.o.sessions?.[0]?.state ?? null,
      statusDuringExit: stDuring.code,
      doctorExit: doctor.code,
      doctorOk: doctorObj?.ok ?? null,
      doctorChecks: checks,
      sessionsAfterClaudeTotal: last(after)?.sessions_total ?? null,
      stopExit: stop.code,
      stopStopped: last(stop)?.stopped ?? null,
      statusAfterExit: sAfter.code,
      sessionsAfterExit: sessAfter.code,
      logPathKnown: Boolean(logPath),
      logTailLines: logLines,
      withdrawalsSentLogged: withdrawn,
      sleepFailed,
      holdGapMs,
      holdKept: !sleepFailed && holdGapMs !== null && holdGapMs >= hold * 1000,
    };
    ctx.record('observed', observed);

    const expect = (ok, text) => ok || ctx.finding(text);
    expect(observed.startExit === 0, `oac start --detach exited ${observed.startExit}`);
    expect(observed.statusExit === 0 && observed.statusRunning, 'oac status did not report a running daemon');
    expect(observed.sessionSeen, 'a real Claude Code session did not appear in oac sessions');
    expect(observed.doctorExit === 0 && observed.doctorOk === true, `oac doctor exited ${observed.doctorExit}`);
    expect(observed.stopExit === 0 && observed.stopStopped === true, `oac stop exited ${observed.stopExit}`);
    expect(observed.statusAfterExit === 3 && observed.sessionsAfterExit === 3, 'status or sessions after stop did not exit 3');
    expect(observed.withdrawalsSentLogged, 'the daemon log has no "shut down: withdrawals sent" line');
    const good =
      observed.startExit === 0 && observed.statusRunning && observed.sessionSeen && observed.doctorExit === 0 &&
      observed.stopExit === 0 && observed.stopStopped === true && observed.statusAfterExit === 3 &&
      observed.sessionsAfterExit === 3 && observed.withdrawalsSentLogged;
    if (!good) throw new Error('G10 live expectations not met; see the findings and the captures');
  }
}
