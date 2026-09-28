// Self-test scenario: a wait issued without a timeout must be refused before it runs.
export default {
  name: 'selftest-unbounded-wait',
  harnesses: [],
  defaults: { launch: [], timeboxMs: 60000, params: {} },
  async run(ctx) {
    const ws = await ctx.herdr.workspaceCreate({ cwd: ctx.dir('cwd') });
    await ctx.herdr.exec('wait', ['pane', 'wait-output', ws.paneId, '--match', 'never']);
  },
};
