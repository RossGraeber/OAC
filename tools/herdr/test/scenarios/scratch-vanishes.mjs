// Self-test scenario (#239): the run's scratch directory -- herdr's working directory -- is
// removed under the run while a fake Claude Code (test/fake-claude.mjs) sits in a dialog that
// ignores keys, so it never exits by itself. The next herdr call cannot even be spawned
// (ENOENT) and the run fails; teardown must still query the pane, stop and delete the session
// and leave no pane process running. Before the fix the first teardown herdr call threw and
// the server, the pane shell and the fake Claude all outlived the run. Self-test only: the
// directory removed is this run's own scratch (the parent of a ctx.dir()).
import { rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';

export default {
  name: 'selftest-scratch-vanishes',
  description: 'self-test only',
  harnesses: [],
  defaults: { launch: ['claude'], timeboxMs: 60000, params: {} },
  async run(ctx) {
    const { herdr } = ctx;
    const cwd = ctx.dir('cwd');
    writeFileSync(join(cwd, '.mcp.json'), JSON.stringify({ mcpServers: {} }));
    const ws = await herdr.workspaceCreate({ cwd });
    ctx.record('paneId', ws.paneId);
    const started = await ctx.startAgent('stuck', { paneId: ws.paneId, timeoutMs: 20000, allowErrorCodes: ['agent_not_ready'] });
    ctx.record('agentStartErrorCode', started.errorCode);
    rmSync(dirname(cwd), { recursive: true, force: true });
    await herdr.agentRead('stuck');
  },
};
