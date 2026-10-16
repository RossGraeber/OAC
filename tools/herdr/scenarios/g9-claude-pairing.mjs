// g9-claude-pairing: G9 (#70) slice 7, leg A. NOT VERDICT-BEARING (G9 is not a gate).
//
// A real Claude Code (print mode, so no dialog) is started by herdr in a pane, with the OAC shim as its
// MCP server and the OAC hook handler on SessionStart, against the example live daemon (cli/examples/
// live_daemon.rs: the product's Daemon::start with an in-memory key, the in-memory transport and a test
// namespace, so nothing of the user's device key is touched). It shows, on the machine it runs on:
//   - the ancestry and pairing: the shim and the hook handler both reach the same `claude` process
//     (the daemon's `accepted` events: the peer's process chain, the harness process it found, paired);
//   - the named-pipe (Windows) or socket (Unix) peer authentication in a real launch: both clients
//     connected, so each passed its check of the server's owner and access list, and the daemon
//     accepted each as this user's;
//   - the hello line and the binding: the real Claude's call of the `oac` server's `whoami` returns a
//     session id (the hello's E equals the hook's id: case 3(a));
//   - whether OAC_SCOPE reaches the MCP server and the hook command through Claude Code (11-risks row 19):
//     with --param explicitEnv=0 (the default) it is set only in the pane's environment, so a shim that
//     connects proves the inheritance; with explicitEnv=1 it is also set in the MCP server's `env`.
//
// No project files are written (the MCP server and the hook come from --mcp-config / --strict-mcp-config
// and --settings files in scratch), so Claude Code shows no trust or MCP-approval dialog; no credential is
// read and the harness config is not edited. The scenario does not accept any dialog: print mode shows none.
//
// Operator command:
//   node tools/herdr/run.mjs --scenario g9-claude-pairing --param oac=<abs path to oac(.exe)> \
//     --param daemon=<abs path to the live_daemon example> [--param namespace=lega] [--param explicitEnv=0|1] \
//     --out <run dir>

import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { randomBytes } from 'node:crypto';

import { regexLiteral } from '../lib/pane-shell.mjs';

function jsonLines(text) {
  const out = [];
  for (const line of text.split(/\r?\n/)) {
    if (!line.trim().startsWith('{')) continue;
    try {
      out.push(JSON.parse(line));
    } catch {
      /* a partial line */
    }
  }
  return out;
}

export default {
  name: 'g9-claude-pairing',
  description: 'Real Claude Code (print mode) + OAC shim and hook + the example live daemon: ancestry, pairing, binding.',
  harnesses: ['claude'],
  defaults: {
    launch: [],
    timeboxMs: 600000,
    params: { oac: '', daemon: '', namespace: 'lega', explicitEnv: '0', waitMs: '60000', claudeWaitMs: '240000' },
  },
  async run(ctx) {
    const { herdr, params } = ctx;
    if (!params.oac || !params.daemon) throw new Error('--param oac=<path> and --param daemon=<path> are required');
    const ns = params.namespace;
    if (!/^[a-z0-9]{1,24}$/.test(ns)) throw new Error('namespace must be 1 to 24 lower-case letters and digits');
    const waitMs = Number(params.waitMs);
    const claudeWaitMs = Number(params.claudeWaitMs);
    const explicitEnv = params.explicitEnv === '1';
    const work = ctx.dir('g9');
    const events = join(work, 'events.jsonl');
    const mcpPath = join(work, 'mcp.json');
    const settingsPath = join(work, 'settings.json');
    const slash = (p) => p.replace(/\\/g, '/');

    // The MCP server and the hook handler. The hook command is a plain path (no shell syntax) so
    // that bash, cmd and PowerShell all run it; OAC_SCOPE comes from the pane's environment.
    const server = { command: params.oac, args: ['mcp-shim', '--harness', 'claude'] };
    if (explicitEnv) server.env = { OAC_SCOPE: ns };
    writeFileSync(mcpPath, JSON.stringify({ mcpServers: { oac: server } }));
    writeFileSync(
      settingsPath,
      JSON.stringify({ hooks: { SessionStart: [{ hooks: [{ type: 'command', command: `"${slash(params.oac)}" hook` }] }] } }),
    );
    ctx.record('mcpConfig', { explicitEnv, server: { command: 'oac', args: server.args } });
    ctx.record('g9', { nonVerdictBearing: 'G9 #70 slice 7 leg A: not a gate; records what the daemon observed, and never changes a gate verdict' });

    // Pane A: the live daemon, in the foreground.
    const wsA = await herdr.workspaceCreate({ cwd: work, label: 'oac-g9-daemon', env: { OAC_SCOPE: ns } });
    ctx.record('daemonPane', wsA);
    await herdr.paneRun(wsA.paneId, ctx.quoteForPane([params.daemon, '--namespace', ns, '--events', events]));
    try {
      await herdr.paneWaitOutput(wsA.paneId, { regex: regexLiteral('"event":"ready"'), timeoutMs: waitMs });
    } catch (err) {
      ctx.capture('pane-daemon-not-ready.txt', await herdr.paneRead(wsA.paneId, { source: 'recent-unwrapped', lines: 200 }));
      throw err;
    }

    // Pane B: the real Claude Code, with OAC_SCOPE in its environment only (unless explicitEnv).
    const wsB = await herdr.workspaceCreate({ cwd: ctx.dir('claude-cwd'), label: 'oac-g9-claude', env: { OAC_SCOPE: ns } });
    ctx.record('claudePane', wsB);
    await ctx.probeEnv(wsB.paneId, { timeoutMs: waitMs });
    const mark = `LEGA-DONE-${randomBytes(4).toString('hex')}`;
    const claude = ctx.quoteForPane([
      'claude',
      '-p',
      'Call the whoami tool of the oac MCP server once and then print its result text exactly, nothing else.',
      '--allowedTools',
      'mcp__oac__whoami',
      '--mcp-config',
      mcpPath,
      '--strict-mcp-config',
      '--settings',
      settingsPath,
      '--max-turns',
      '4',
    ]);
    await herdr.paneRun(wsB.paneId, `${claude}; echo ${mark}`);
    try {
      await herdr.paneWaitOutput(wsB.paneId, { regex: `^\\s*${regexLiteral(mark)}\\s*$`, timeoutMs: claudeWaitMs });
    } catch (err) {
      ctx.capture('pane-claude-not-done.txt', await herdr.paneRead(wsB.paneId, { source: 'recent-unwrapped', lines: 200 }));
      throw err;
    }
    const claudeText = await herdr.paneRead(wsB.paneId, { source: 'recent-unwrapped', lines: 200 });
    ctx.capture('pane-claude.txt', claudeText);
    await herdr.paneProcessInfo(wsB.paneId);

    // Stop the daemon: a `stop` line on its standard input, typed into its pane.
    await herdr.paneRun(wsA.paneId, 'stop');
    await herdr.paneWaitOutput(wsA.paneId, { regex: regexLiteral('"event":"stopped"'), timeoutMs: waitMs });
    ctx.capture('pane-daemon.txt', await herdr.paneRead(wsA.paneId, { source: 'recent-unwrapped', lines: 200 }));
    await herdr.paneProcessInfo(wsA.paneId);

    // What the daemon observed.
    const text = readFileSync(events, 'utf8');
    ctx.capture('events.jsonl.txt', text);
    const all = jsonLines(text);
    const accepted = all.filter((e) => e.event === 'accepted');
    const ended = all.filter((e) => e.event === 'ended');
    const logs = all.filter((e) => e.event === 'log').map((e) => e.line);
    const harnessPids = new Set(accepted.map((e) => e.harness?.pid ?? null));
    const summary = {
      accepted: accepted.length,
      ended: ended.length,
      allPaired: accepted.length > 0 && accepted.every((e) => e.paired === true),
      sameHarnessProcess: harnessPids.size === 1 && !harnessPids.has(null),
      ancestry: accepted.map((e) => ({
        peer: e.peer?.pid,
        chain: (e.chain ?? []).map((c) => c.image).slice(0, 8),
        harness: e.harness?.pid ?? null,
      })),
      bindingLines: logs.filter((l) => /binding/.test(l)),
      stopped: all.some((e) => e.event === 'stopped'),
      whoamiSessionId: /"session_id"\s*:\s*"[0-9a-z]+"/.test(claudeText),
      whoamiUnauthorized: /unauthorized/i.test(claudeText),
    };
    ctx.record('observed', summary);

    if (accepted.length < 2) ctx.finding(`only ${accepted.length} connection(s) reached the daemon: expected the shim and the hook handler`);
    if (!summary.allPaired || !summary.sameHarnessProcess) ctx.finding('the shim and the hook did not both pair to one harness process');
    if (!summary.whoamiSessionId) ctx.finding('the real Claude Code did not show a bound session id from the whoami tool');
    if (!(accepted.length >= 2 && summary.allPaired && summary.sameHarnessProcess && summary.whoamiSessionId && summary.stopped)) {
      throw new Error('G9 leg A expectations not met; see the findings and the captures');
    }
  },
};
