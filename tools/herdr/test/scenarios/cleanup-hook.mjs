// Self-test scenario: registers a cleanup hook (ctx.addCleanup) that writes a marker file, then awaits something that
// is not herdr (a plain timer) for --param sleepMs, like slow.mjs. The timebox ends the run while the scenario is
// still awaiting, so its own code never gets to undo anything: the hook must still run, from run.mjs's teardown.
// It records `awaiting` once it is about to sleep (see slow.mjs: a box already spent in start-up never gets here).
import { writeFileSync } from 'node:fs';

export default {
  name: 'selftest-cleanup-hook',
  harnesses: [],
  defaults: { launch: [], timeboxMs: 60000, params: { sleepMs: '60000', marker: '' } },
  async run(ctx) {
    ctx.addCleanup('write the marker', async () => {
      writeFileSync(ctx.params.marker, 'ran');
    });
    ctx.addCleanup('a hook that throws', () => {
      throw new Error('this hook fails on purpose');
    });
    const ws = await ctx.herdr.workspaceCreate({ cwd: ctx.dir('cwd') });
    await ctx.herdr.paneProcessInfo(ws.paneId);
    ctx.record('awaiting', { remainingMs: ctx.remainingMs() });
    await new Promise((res) => setTimeout(res, Number(ctx.params.sleepMs)));
  },
};
