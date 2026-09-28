// Self-test scenario: awaits something that is not herdr (a plain timer) for --param
// sleepMs. The timebox and an operator abort must still end the run.
export default {
  name: 'selftest-slow',
  harnesses: [],
  defaults: { launch: [], timeboxMs: 60000, params: { sleepMs: '60000' } },
  async run(ctx) {
    const ws = await ctx.herdr.workspaceCreate({ cwd: ctx.dir('cwd') });
    await ctx.herdr.paneProcessInfo(ws.paneId);
    await new Promise((res) => setTimeout(res, Number(ctx.params.sleepMs)));
  },
};
