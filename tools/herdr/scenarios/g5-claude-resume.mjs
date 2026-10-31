// g5-claude-resume: G5 (#66) box 4, the live check of registration across start, exit, --resume and
// --continue. NOT VERDICT-BEARING (G5 is not a gate).
//
// A real Claude Code (print mode, so no dialog) is started by herdr three times in one working
// directory, each time with the OAC shim as its MCP server and the OAC hook handler on SessionStart,
// against the example live daemon (cli/examples/live_daemon.rs: an in-memory key, the in-memory
// transport and a test namespace, so nothing of the user's device key is touched):
//   run A: a fresh session;
//   run B: `claude -p --resume <run A's Claude session id>`;
//   run C: `claude -p --continue`.
// Each run is held alive by one allowed Bash call (`sleep N`) so that `oac sessions --json` (in the
// test namespace) can list it, and `oac sessions --json` is read again after each run ends. It shows,
// on the machine it runs on:
//   - registration: each run is listed as one bound session while it is alive;
//   - deregistration: after each run exits, no session is listed (SC-ID-155);
//   - the C4 section 6 rule, as the fake-harness test registration.rs asserts it: a resumed or
//     continued Claude session is a NEW OAC session (its OAC session id differs from every earlier one);
//   - what Claude Code itself reported as its session id for each run (whether --resume and --continue
//     kept run A's), from its `--output-format json` result. Recorded, not judged: OAC does not depend on it.
//
// No project files are written (the MCP server and the hook come from --mcp-config / --strict-mcp-config
// and --settings files in scratch), so Claude Code shows no trust or MCP-approval dialog; no credential is
// read and the harness config is not edited. The scenario accepts no dialog: print mode shows none.
// OAC session ids and Claude session ids are compared in memory and redacted in the captures.
//
// Operator command:
//   node tools/herdr/run.mjs --scenario g5-claude-resume --param oac=<abs path to oac(.exe)> \
//     --param daemon=<abs path to the live_daemon example> [--param namespace=g5r] \
//     [--param holdSeconds=30] --out <run dir>

import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { randomBytes } from 'node:crypto';

import { regexLiteral } from '../lib/pane-shell.mjs';

function jsonLines(text) {
  const out = [];
  for (const line of text.split(/\r?\n/)) {
    const t = line.trim();
    if (!t.startsWith('{')) continue;
    try {
      out.push(JSON.parse(t));
    } catch {
      /* a partial line */
    }
  }
  return out;
}

export default {
  name: 'g5-claude-resume',
  description: 'Real Claude Code (print mode) started, resumed and continued against the example live daemon: registration and deregistration.',
  harnesses: ['claude'],
  defaults: {
    launch: [],
    timeboxMs: 1200000,
    params: { oac: '', daemon: '', namespace: 'g5r', holdSeconds: '30', waitMs: '60000', claudeWaitMs: '240000', pollAttempts: '20' },
  },
  async run(ctx) {
    const { herdr, params } = ctx;
    if (!params.oac || !params.daemon) throw new Error('--param oac=<path> and --param daemon=<path> are required');
    const ns = params.namespace;
    if (!/^[a-z0-9]{1,24}$/.test(ns)) throw new Error('namespace must be 1 to 24 lower-case letters and digits');
    const waitMs = Number(params.waitMs);
    const claudeWaitMs = Number(params.claudeWaitMs);
    const hold = Number(params.holdSeconds);
    const attempts = Number(params.pollAttempts);
    if (!Number.isInteger(hold) || hold < 10 || hold > 300) throw new Error('holdSeconds must be an integer from 10 to 300');
    const work = ctx.dir('g5r');
    const events = join(work, 'events.jsonl');
    const mcpPath = join(work, 'mcp.json');
    const settingsPath = join(work, 'settings.json');
    const slash = (p) => p.replace(/\\/g, '/');
    const exitVar = ctx.paneShell === 'powershell' ? '$LASTEXITCODE' : '$?';
    const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

    writeFileSync(mcpPath, JSON.stringify({ mcpServers: { oac: { command: params.oac, args: ['mcp-shim', '--harness', 'claude'] } } }));
    writeFileSync(
      settingsPath,
      JSON.stringify({ hooks: { SessionStart: [{ hooks: [{ type: 'command', command: `"${slash(params.oac)}" hook` }] }] } }),
    );
    ctx.record('g5', { nonVerdictBearing: 'G5 #66 box 4: not a gate; records what the daemon and oac sessions reported, and never changes a gate verdict' });

    // Pane A: the live daemon, in the foreground.
    const wsA = await herdr.workspaceCreate({ cwd: work, label: 'oac-g5r-daemon', env: { OAC_SCOPE: ns } });
    ctx.record('daemonPane', wsA);
    await herdr.paneRun(wsA.paneId, ctx.quoteForPane([params.daemon, '--namespace', ns, '--events', events]));
    try {
      await herdr.paneWaitOutput(wsA.paneId, { regex: regexLiteral('"event":"ready"'), timeoutMs: waitMs });
    } catch (err) {
      ctx.capture('pane-daemon-not-ready.txt', await herdr.paneRead(wsA.paneId, { source: 'recent-unwrapped', lines: 200 }));
      throw err;
    }
    const stopDaemon = async () => {
      await herdr.paneRun(wsA.paneId, 'stop');
      await herdr.paneWaitOutput(wsA.paneId, { regex: regexLiteral('"event":"stopped"'), timeoutMs: waitMs });
    };

    try {
      // Pane O: `oac sessions --json` in the test namespace.
      const wsO = await herdr.workspaceCreate({ cwd: work, label: 'oac-g5r-ops', env: { OAC_SCOPE: ns } });
      ctx.record('opsPane', wsO);
      let n = 0;
      async function sessions(tag) {
        n += 1;
        const id = `${tag}${n}-${randomBytes(3).toString('hex')}`;
        const cmd = ctx.quoteForPane([params.oac, 'sessions', '--json']);
        await herdr.paneRun(wsO.paneId, `echo G5R-${id}-BEGIN; ${cmd}; echo G5R-${id}-EXIT-${exitVar}`);
        await herdr.paneWaitOutput(wsO.paneId, { regex: `^\\s*G5R-${regexLiteral(id)}-EXIT-\\d+\\s*$`, timeoutMs: waitMs });
        const lines = (await herdr.paneRead(wsO.paneId, { source: 'recent-unwrapped', lines: 400 })).split(/\r?\n/);
        const b = lines.findIndex((l) => new RegExp(`^\\s*G5R-${regexLiteral(id)}-BEGIN\\s*$`).test(l));
        const e = lines.findIndex((l) => new RegExp(`^\\s*G5R-${regexLiteral(id)}-EXIT-\\d+\\s*$`).test(l));
        if (b < 0 || e < b) throw new Error(`sessions ${id}: markers not found in the pane text`);
        const text = lines.slice(b + 1, e).join('\n');
        const code = Number(lines[e].trim().split('-').pop());
        const o = jsonLines(text).filter((x) => x && x.oac_cli === 1).at(-1) ?? null;
        return { code, text, o, n };
      }

      // Pane B: the real Claude Code, one working directory for all three runs.
      const wsB = await herdr.workspaceCreate({ cwd: ctx.dir('claude-cwd'), label: 'oac-g5r-claude', env: { OAC_SCOPE: ns } });
      ctx.record('claudePane', wsB);
      await ctx.probeEnv(wsB.paneId, { timeoutMs: waitMs });
      // No double quote in the prompt (PowerShell 5.1 drops it; see g10-cli-verbs).
      const prompt =
        `Call the whoami tool of the oac MCP server once. Then run the Bash command sleep ${hold} (exactly that command). ` +
        'Then print the whoami result text exactly, nothing else.';
      if (ctx.paneShell === 'powershell' && prompt.includes('"')) throw new Error('the prompt must not contain a double quote under the PowerShell pane shell');

      const runs = [];
      const captures = [];
      async function claudeRun(label, extra) {
        const mark = `G5R-${label}-DONE-${randomBytes(4).toString('hex')}`;
        const claude = ctx.quoteForPane([
          'claude', '-p', prompt, ...extra,
          '--output-format', 'json',
          '--allowedTools', 'mcp__oac__whoami', 'Bash(sleep *)',
          '--mcp-config', mcpPath, '--strict-mcp-config', '--settings', settingsPath,
          '--max-turns', '6',
        ]);
        await herdr.paneRun(wsB.paneId, `${claude}; echo ${mark}`);

        // While it is alive: poll (bounded) until one session is listed.
        let during = null;
        for (let i = 0; i < attempts && !during; i += 1) {
          await sleep(3000);
          const r = await sessions(`${label}-during`);
          captures.push({ tag: `${label}-during`, r });
          if (r.code === 0 && r.o && r.o.sessions_total >= 1) during = r;
        }
        try {
          await herdr.paneWaitOutput(wsB.paneId, { regex: `^\\s*${regexLiteral(mark)}\\s*$`, timeoutMs: claudeWaitMs });
        } catch (err) {
          ctx.capture(`pane-claude-${label}-not-done.txt`, await herdr.paneRead(wsB.paneId, { source: 'recent-unwrapped', lines: 200 }));
          throw err;
        }
        const claudeText = await herdr.paneRead(wsB.paneId, { source: 'recent-unwrapped', lines: 200 });
        const results = jsonLines(claudeText).filter((x) => x && x.type === 'result' && typeof x.session_id === 'string');
        const claudeSessionId = results.at(-1)?.session_id ?? null;
        await sleep(2000);
        const after = await sessions(`${label}-after`);
        captures.push({ tag: `${label}-after`, r: after });
        const listed = during?.o?.sessions ?? [];
        const run = {
          label,
          listedDuring: during ? during.o.sessions_total : 0,
          oacSessionId: listed.length === 1 ? listed[0].session_id : null,
          harness: listed[0]?.harness ?? null,
          state: listed[0]?.state ?? null,
          totalAfter: after.o?.sessions_total ?? null,
          claudeSessionId,
          sleepFailed: /missing operand/i.test(claudeText),
          claudeText,
        };
        runs.push(run);
        return run;
      }

      const a = await claudeRun('A', []);
      if (!a.claudeSessionId) throw new Error('run A: no Claude session id in its --output-format json result; cannot resume');
      await claudeRun('B', ['--resume', a.claudeSessionId]);
      await claudeRun('C', ['--continue']);

      // Redact before any capture is written: OAC ids and Claude ids.
      runs.forEach((r) => {
        if (r.oacSessionId) ctx.redactLiteral(r.oacSessionId, `<OAC_SESSION_${r.label}>`);
      });
      const claudeIds = [...new Set(runs.map((r) => r.claudeSessionId).filter(Boolean))];
      claudeIds.forEach((id, i) => ctx.redactLiteral(id, `<CLAUDE_SESSION_${i + 1}>`));
      for (const r of runs) ctx.capture(`pane-claude-${r.label}.txt`, r.claudeText);
      for (const c of captures) ctx.capture(`sessions-${c.r.n}-${c.tag}.txt`, `$ oac sessions --json\n${c.r.text}\nexit ${c.r.code}\n`);

      const oacIds = runs.map((r) => r.oacSessionId);
      const summary = {
        runs: runs.map((r) => ({
          label: r.label,
          listedDuring: r.listedDuring,
          bound: Boolean(r.oacSessionId),
          harness: r.harness,
          state: r.state,
          totalAfter: r.totalAfter,
          sleepFailed: r.sleepFailed,
          claudeSessionIsRunA: r.claudeSessionId === null ? null : r.claudeSessionId === a.claudeSessionId,
        })),
        oacSessionIdsDistinct: oacIds.every(Boolean) && new Set(oacIds).size === oacIds.length,
      };
      ctx.record('observed', summary);

      for (const r of runs) {
        if (r.sleepFailed) ctx.finding(`run ${r.label}: the held Bash call failed; the session was not held`);
        if (r.listedDuring !== 1 || !r.oacSessionId) ctx.finding(`run ${r.label}: oac sessions did not list exactly one bound session while it was alive (listed ${r.listedDuring})`);
        if (r.totalAfter !== 0) ctx.finding(`run ${r.label}: ${r.totalAfter} session(s) still listed after it exited`);
      }
      if (!summary.oacSessionIdsDistinct) ctx.finding('the three runs did not get three distinct OAC session ids (C4 section 6: a resumed or continued session is a new session)');
      const ok = runs.every((r) => !r.sleepFailed && r.listedDuring === 1 && r.oacSessionId && r.totalAfter === 0) && summary.oacSessionIdsDistinct;

      await stopDaemon();
      ctx.capture('pane-daemon.txt', await herdr.paneRead(wsA.paneId, { source: 'recent-unwrapped', lines: 200 }));
      const all = jsonLines(readFileSync(events, 'utf8'));
      ctx.capture('events.jsonl.txt', readFileSync(events, 'utf8'));
      ctx.record('daemon', {
        accepted: all.filter((e) => e.event === 'accepted').length,
        ended: all.filter((e) => e.event === 'ended').length,
        bindingLines: all.filter((e) => e.event === 'log' && /binding/.test(e.line)).map((e) => e.line),
        stopped: all.some((e) => e.event === 'stopped'),
      });
      if (!ok) throw new Error('G5 resume leg expectations not met; see the findings and the captures');
    } catch (err) {
      try {
        await stopDaemon();
      } catch {
        /* already stopped, or the pane is gone */
      }
      throw err;
    }
  },
};
