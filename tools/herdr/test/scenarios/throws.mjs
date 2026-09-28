// Self-test scenario: fails after creating a pane; teardown must still run.
export default {
  name: 'selftest-throws',
  harnesses: [],
  defaults: { launch: [], timeboxMs: 60000, params: {} },
  async run(ctx) {
    const ws = await ctx.herdr.workspaceCreate({ cwd: ctx.dir('cwd') });
    await ctx.herdr.paneProcessInfo(ws.paneId);
    throw new Error('planted scenario failure');
  },
};
