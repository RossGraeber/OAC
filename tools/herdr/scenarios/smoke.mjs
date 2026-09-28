// smoke: the driver's own end-to-end check, with no harness involved.
//
// Starts the run's named herdr session (run.mjs does that), opens one plain shell pane in a
// scratch cwd, records the pane's environment delta, types the launch command into the
// shell, waits (bounded) for its expected output line, reads the pane, and captures what
// it read. Teardown (run.mjs) then stops and deletes the session and verifies no herdr
// server or pane process is left running.
//
// Launch is a parameter: `--launch '<json argv>'` is typed into the pane's shell, quoted
// for `--param paneShell=posix|powershell|cmd`. `--param expect=<line>` is the whole output
// line to wait for; it is matched with an anchored regex so the echoed command line itself
// can never satisfy the wait. An empty launch (`--launch '[]'`) only reads the pane.

import { regexLiteral } from '../lib/pane-shell.mjs';

export default {
  name: 'smoke',
  description: 'Plain shell pane: open, probe env, run the launch command, read, tear down; no harness.',
  harnesses: [],
  defaults: {
    launch: ['echo', 'OAC-SMOKE-READY'],
    timeboxMs: 120000,
    params: { expect: 'OAC-SMOKE-READY', waitMs: '20000' },
  },
  async run(ctx) {
    const { herdr, launch, params } = ctx;
    const waitMs = Number(params.waitMs);

    const ws = await herdr.workspaceCreate({ cwd: ctx.dir('pane-cwd'), label: 'oac-smoke' });
    ctx.record('workspace', ws);
    ctx.record('paneProcessInfo', await herdr.paneProcessInfo(ws.paneId));
    await ctx.probeEnv(ws.paneId, { timeoutMs: waitMs });

    if (launch.length) {
      await herdr.paneRun(ws.paneId, ctx.quoteForPane(launch));
      await herdr.paneWaitOutput(ws.paneId, { regex: `^\\s*${regexLiteral(params.expect)}\\s*$`, timeoutMs: waitMs });
    }

    const text = await herdr.paneRead(ws.paneId, { source: 'recent-unwrapped', lines: 200 });
    ctx.capture('pane-smoke.txt', text);
    if (launch.length && !text.split(/\r?\n/).some((l) => l.trim() === params.expect)) {
      throw new Error('pane read does not show the expected output line');
    }
    // Re-read the pane's processes so teardown can check every pid seen during the run.
    await herdr.paneProcessInfo(ws.paneId);
  },
};
