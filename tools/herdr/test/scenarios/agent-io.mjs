// Self-test scenario: agent read / send-keys / prompt paths against the fake herdr.
// --param op=read|send-keys|prompt-retry|dialog-no-read|dialog-after-read|dialog-after-failed-read
//            |dialog-after-pane-input|dialog-after-pane-read (#139)
import { DriverError, NotRunError } from '../../lib/herdr.mjs';

export default {
  name: 'selftest-agent-io',
  description: 'self-test only',
  harnesses: [],
  defaults: { launch: ['codex'], timeboxMs: 60000, params: { op: 'read', deadlineMs: '1500' } },
  async run(ctx) {
    const { herdr, params } = ctx;
    const ws = await herdr.workspaceCreate({ cwd: ctx.dir('agent-cwd'), label: 'selftest' });
    await herdr.paneProcessInfo(ws.paneId);
    await ctx.startAgent('selftest', { paneId: ws.paneId, timeoutMs: 10000 });
    const deadlineMs = Number(params.deadlineMs);
    if (params.op === 'read') await herdr.agentRead('selftest', { deadlineMs });
    else if (params.op === 'send-keys') await herdr.agentSendKeys('selftest', ['enter'], { deadlineMs });
    else if (params.op === 'dialog-no-read') await herdr.dialogAccept('selftest');
    else if (params.op === 'dialog-after-failed-read') {
      // A read that FAILED must not unlock dialog-accept.
      try {
        await herdr.agentRead('selftest', { deadlineMs });
      } catch (err) {
        if (!(err instanceof DriverError)) throw err;
      }
      await herdr.dialogAccept('selftest');
    }
    else if (params.op === 'dialog-after-pane-input') {
      // #139: input sent to the agent's pane by pane id between the agent-level read and the
      // accept must reset the read guard.
      await herdr.agentRead('selftest', { deadlineMs });
      await herdr.paneRun(ws.paneId, 'echo typed-between-read-and-accept');
      await herdr.dialogAccept('selftest');
    } else if (params.op === 'dialog-after-pane-read') {
      // #139: a read of the agent's pane by pane id covers the agent.
      ctx.capture('dialog.txt', await herdr.paneRead(ws.paneId));
      await herdr.dialogAccept('selftest');
    } else if (params.op === 'dialog-after-read') {
      ctx.capture('dialog.txt', await herdr.agentRead('selftest', { deadlineMs }));
      await herdr.dialogAccept('selftest');
    } else if (params.op === 'prompt-retry') {
      try {
        await herdr.agentPrompt('selftest', 'first and only prompt', { wait: { until: ['idle'], timeoutMs: 800 } });
      } catch (err) {
        if (!(err instanceof NotRunError)) throw err;
        // A scenario that tries to re-submit after a timeout must be refused by the driver.
        try {
          await herdr.agentPrompt('selftest', 'first and only prompt', { wait: { until: ['idle'], timeoutMs: 800 } });
          ctx.record('retry', 'SENT (driver failed to refuse)');
        } catch (again) {
          ctx.record('retry', again instanceof DriverError ? `refused: ${again.message}` : `unexpected: ${again.message}`);
        }
        throw err;
      }
    } else throw new Error(`unknown op ${params.op}`);
  },
};
