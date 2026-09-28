// Self-test scenario: the scenario catches a herdr timeout itself. The driver must still
// record the run as NOT RUN (--param then=return|throw).
import { NotRunError } from '../../lib/herdr.mjs';

export default {
  name: 'selftest-catches-timeout',
  harnesses: [],
  defaults: { launch: ['codex'], timeboxMs: 60000, params: { then: 'return' } },
  async run(ctx) {
    const { herdr, params } = ctx;
    const ws = await herdr.workspaceCreate({ cwd: ctx.dir('cwd') });
    await herdr.paneProcessInfo(ws.paneId);
    await ctx.startAgent('selftest', { paneId: ws.paneId, timeoutMs: 10000 });
    try {
      await herdr.agentPrompt('selftest', 'only prompt', { wait: { until: ['idle'], timeoutMs: 800 } });
    } catch (err) {
      if (!(err instanceof NotRunError)) throw err;
      ctx.record('caught', err.reason);
      if (params.then === 'throw') throw new Error('unrelated failure after catching the timeout');
    }
  },
};
