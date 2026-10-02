// Self-test scenario (#136): opens a pane and never calls paneProcessInfo, so the driver
// knows the pane only from workspaceCreate. Teardown must still find, report and kill the
// pane's processes if the session stop leaves them running.
export default {
  name: 'selftest-no-process-info',
  harnesses: [],
  defaults: { launch: [], timeboxMs: 60000, params: {} },
  async run(ctx) {
    const ws = await ctx.herdr.workspaceCreate({ cwd: ctx.dir('cwd') });
    ctx.record('paneId', ws.paneId);
  },
};
