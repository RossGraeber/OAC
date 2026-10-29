// The driver's own handling of Codex for the G8 S6 live leg (scenarios/g8-codex-pairing.mjs), under
// the lead's decision of 2026-10-29: the driver registers the OAC MCP server with the Codex CLI, runs
// the Codex TUI in a herdr pane, and answers a short, fixed list of its screens, as it already did for
// Claude Code (#196). Everything outside that list is refused and ends the run NOT RUN.
//
//   - Registration: `codex mcp add` at setup (after a `codex mcp remove` that tolerates "not found"),
//     `codex mcp remove` in teardown on every path. Each command and its output is recorded. The driver
//     never reads config.toml; `codex mcp get` verifies, and its env values stay as Codex prints them.
//   - Dialogs: the table is lib/g2.mjs CODEX_DIALOG_KINDS with driverMayAcceptCodex: the folder-trust
//     screen of the project directory ("Trust and continue"), the update prompt ("Skip"), and the MCP
//     tool-approval prompt for the registered server and OAC's four tools ("1. Allow", this call only,
//     never "Allow for this session" or "Always allow"). The startup hook review, a sign-in, an
//     elicitation, a sandbox or exec approval, an approval naming another server: refused.
//
// Nothing here reads a file under the Codex home, starts or stops the shared app-server daemon, calls
// turn/steer or answers a server request.

import { basename } from 'node:path';

import { classifyCodexScreen, driverMayAcceptCodex } from './g2.mjs';

// --- the tools the driver may allow ---------------------------------------------------------------------

// The four OAC tools and their declared inputs: adapters/mcp-tools/src/lib.rs `tools()` (MCPB-TOOL-001).
// The tool-approval prompt lists the arguments of the call; each name must be one of these.
export const OAC_TOOL_INPUTS = Object.freeze({
  send: Object.freeze(['to', 'content', 'conversation_id', 'correlation_id']),
  reply: Object.freeze(['in_reply_to', 'content', 'to']),
  list_sessions: Object.freeze([]),
  whoami: Object.freeze([]),
});

// What lib/g2.mjs planCodexToolApproval takes: the one server this run registered and its tools.
export const liveToolApproval = (serverName) => ({ server: serverName, tools: Object.keys(OAC_TOOL_INPUTS), arguments: OAC_TOOL_INPUTS });

// --- the dialog table, bound to this run -------------------------------------------------------------------

// classify: the Codex classification with the screen text kept, so the trust screen can be checked
// against the project directory. `onHookFailed` is called once per read that shows a `Hook failed` line
// (a finding, not a dialog).
export function classifyForLive(text, { busyIndicator = 'esc to interrupt', onHookFailed = null } = {}) {
  if (onHookFailed && /Hook failed\b/i.test(String(text ?? ''))) onHookFailed();
  return { ...classifyCodexScreen(text, { busyIndicator }), text: String(text ?? '') };
}

const norm = (s) => String(s ?? '').replace(/\\/g, '/').toLowerCase();

// driverMayAccept for the live leg. Same result shape as planDriverAccept.
export const driverMayAcceptLive = (getExpected, projectDir) => (classification) => {
  const plan = driverMayAcceptCodex(classification, { toolApproval: getExpected() ?? null });
  if (plan.ok && classification?.dialog === 'workspace-trust') {
    // The trust screen names the directory it is about; only the project directory's is accepted.
    const leaf = norm(basename(String(projectDir ?? '').replace(/[\\/]+$/, '')));
    if (!leaf || !norm(classification.text).includes(leaf)) {
      return { ...plan, ok: false, keys: [], moves: [], why: `Codex's folder-trust screen does not name the project directory (${leaf || 'none given'}); the driver trusts only that directory` };
    }
  }
  return plan;
};

// --- registration ---------------------------------------------------------------------------------------------

// The `codex mcp` argvs (after `codex`). No `--`-less form: the command after `--` is oac with its arguments.
export const registrationArgv = {
  get: (name) => ['mcp', 'get', name],
  remove: (name) => ['mcp', 'remove', name],
  add: (name, ns, oac) => ['mcp', 'add', name, '--env', `OAC_SCOPE=${ns}`, '--', oac, 'mcp-shim', '--harness', 'codex'],
};

const cap = (s, n = 1500) => {
  const t = String(s ?? '');
  return t.length > n ? `${t.slice(0, n)}...(${t.length - n} more characters)` : t;
};

// The registration of the OAC MCP server with the Codex CLI. `run(argv)` -> { exitCode, stdout, stderr,
// timedOut, spawnError } runs `codex <argv>`; `record` receives the list of commands; `finding` a string;
// `changesConfig(why, fn)` wraps a command that changes the Codex configuration (run.mjs
// ctx.driverChangesHarnessConfig). unregister() never throws, is safe to call on any path (also after a
// failed or partial register) and runs `codex mcp remove` at most once more than needed: it is idempotent.
export function makeRegistration({ run, serverName, ns, oac, finding = () => {}, changesConfig = async (_why, fn) => fn() }) {
  const commands = [];
  let attempted = false;
  let removed = false;
  const step = async (label, argv, { change = false } = {}) => {
    const exec = () => run(argv);
    const r = change ? await changesConfig(`${label} (the driver's own registration command)`, exec) : await exec();
    const rec = { label, argv: ['codex', ...argv], exitCode: r.exitCode ?? null, timedOut: !!r.timedOut, spawnError: r.spawnError ?? null, stdout: cap(r.stdout), stderr: cap(r.stderr) };
    commands.push(rec);
    return rec;
  };
  const ok = (r) => r.exitCode === 0 && !r.timedOut && !r.spawnError;
  return {
    commands,
    get attempted() {
      return attempted;
    },
    get removed() {
      return removed;
    },
    // Remove a leftover of the same name (a finding), then add and verify.
    async register() {
      attempted = true;
      const before = await step('get (before)', registrationArgv.get(serverName));
      if (ok(before)) {
        finding(`an MCP server named ${serverName} was already registered with Codex before the run (a leftover of an earlier run); the driver removes it and registers its own`);
      }
      // Removal tolerates "not found": a nonzero exit with the server absent is the normal case.
      await step('remove (before)', registrationArgv.remove(serverName), { change: true });
      const add = await step('add', registrationArgv.add(serverName, ns, oac), { change: true });
      if (!ok(add)) throw new Error(`codex mcp add ${serverName} failed (exit ${add.exitCode ?? add.spawnError ?? 'timeout'})`);
      const after = await step('get (after)', registrationArgv.get(serverName));
      if (!ok(after)) throw new Error(`codex mcp get ${serverName} after the add failed (exit ${after.exitCode ?? after.spawnError ?? 'timeout'})`);
      return after;
    },
    async unregister() {
      if (!attempted || removed) return null;
      try {
        const r = await step('remove (teardown)', registrationArgv.remove(serverName), { change: true });
        const gone = await step('get (after teardown)', registrationArgv.get(serverName));
        removed = !ok(gone); // `get` of a missing server fails: the registration is gone
        if (!removed) finding(`the MCP server ${serverName} is STILL registered with Codex after the teardown removal (exit ${r.exitCode}); remove it by hand: codex mcp remove ${serverName}`);
        return r;
      } catch (err) {
        finding(`the teardown removal of the MCP server ${serverName} failed (${String(err?.message ?? err).slice(0, 120)}); remove it by hand: codex mcp remove ${serverName}`);
        return null;
      }
    },
  };
}
