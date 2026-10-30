// Self-test scenario: awaits something that is not herdr (a plain timer). The timebox and an operator abort must
// still end the run. --param sleepMs=<ms> sleeps that long; sleepMs=past-timebox sleeps until the timebox has
// expired plus --param pastMs (default 200), however long the driver's start-up took: it is ordered against the
// timebox itself (ctx.remainingMs), not against a sleep tuned for an unloaded machine (#466). The scenario records
// `awaiting` once it is about to sleep, so a case can tell "the box was already spent in start-up, the scenario never
// ran" (a loaded machine) from the behaviour it tests.
export default {
  name: 'selftest-slow',
  harnesses: [],
  defaults: { launch: [], timeboxMs: 60000, params: { sleepMs: '60000', pastMs: '200' } },
  async run(ctx) {
    const ws = await ctx.herdr.workspaceCreate({ cwd: ctx.dir('cwd') });
    await ctx.herdr.paneProcessInfo(ws.paneId);
    const left = ctx.remainingMs();
    ctx.record('awaiting', { remainingMs: left });
    const ms = ctx.params.sleepMs === 'past-timebox' ? Math.max(0, left) + Number(ctx.params.pastMs) : Number(ctx.params.sleepMs);
    await new Promise((res) => setTimeout(res, ms));
  },
};
