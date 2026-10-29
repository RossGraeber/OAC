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

import { homedir } from 'node:os';

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

const norm = (s) => String(s ?? '').replace(/\\/g, '/').replace(/\/+$/, '').toLowerCase();

// The directory a Codex folder-trust screen is about: its first non-empty line (the screen starts with the working
// directory; the optional Note block that names the repository root comes after it). Codex may show the home
// directory as `~`. -> the path with `~` expanded, or null.
export function trustScreenDirectory(text, home = homedir()) {
  const first = String(text ?? '').split(/\r?\n/).map((l) => l.trim()).find((l) => l !== '');
  if (!first) return null;
  return /^~(?=[\\/]|$)/.test(first) ? `${home}${first.slice(1)}` : first;
}

// driverMayAccept for the live leg. Same result shape as planDriverAccept. The trust screen is accepted only when the
// directory it shows is exactly the project directory (compared as paths, case and slash style aside): not a substring,
// not a directory of the same name elsewhere.
export const driverMayAcceptLive = (getExpected, projectDir, { home = homedir() } = {}) => (classification) => {
  const plan = driverMayAcceptCodex(classification, { toolApproval: getExpected() ?? null });
  if (plan.ok && classification?.dialog === 'workspace-trust') {
    const shown = trustScreenDirectory(classification.text, home);
    if (!projectDir || shown === null || norm(shown) !== norm(projectDir)) {
      return { ...plan, ok: false, keys: [], moves: [], why: `Codex's folder-trust screen does not show exactly the project directory (it shows ${shown === null ? 'no directory' : 'another path'}); the driver trusts only that directory` };
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

// What `codex mcp get <name>` prints for an entry (Codex 0.165.0: `command: <path>`, `args: <arguments>`, the env values
// masked) against this driver's own entry: command the oac path, arguments exactly `mcp-shim --harness codex`.
export function ownEntryShape(stdout, oac) {
  const line = (re) => re.exec(String(stdout ?? ''))?.[1]?.trim() ?? null;
  const command = line(/^\s*command:\s*(.*)$/m);
  const args = line(/^\s*args:\s*(.*)$/m);
  return { command, args, own: !!command && !!args && norm(command) === norm(oac) && args === 'mcp-shim --harness codex' };
}

// An entry of the same name that is not this driver's own (a different command or arguments): the run does not touch it.
export class RegistrationRefused extends Error {}

// The name must say whose it is: a server this driver may add and remove.
export const REGISTRATION_NAME_PREFIX = 'oac-g8live';
export const registrationNameProblem = (name) => (typeof name === 'string' && name.startsWith(REGISTRATION_NAME_PREFIX) && /^[A-Za-z0-9_-]{1,40}$/.test(name) ? null : `the MCP server name must start with ${REGISTRATION_NAME_PREFIX} (a server this driver may add and remove)`);

// The Codex CLI says this when no server of that name is registered (0.165.0, stderr of `get`, exit 1).
const NOT_REGISTERED = /No MCP server named/i;

// The registration of the OAC MCP server with the Codex CLI. `run(argv)` -> { exitCode, stdout, stderr,
// timedOut, spawnError } runs `codex <argv>`; `finding` takes a string; `changesConfig(why, fn)` wraps a command that
// changes the Codex configuration (run.mjs ctx.driverChangesHarnessConfig). register() refuses (RegistrationRefused,
// the run stops NOT RUN, nothing is removed) when an entry of the name exists that is not this driver's own.
// unregister() never throws, is safe to call on any path (also after a failed or partial register), is single-flight
// and idempotent; it counts the server as removed only when `get` then fails cleanly with "No MCP server named": a
// timeout or a spawn failure is not removed, and says so with the command to run by hand.
export function makeRegistration({ run, serverName, ns, oac, finding = () => {}, changesConfig = async (_why, fn) => fn() }) {
  const commands = [];
  let attempted = false;
  let removed = false;
  let inflight = null;
  const step = async (label, argv, { change = false } = {}) => {
    const exec = () => run(argv);
    const r = change ? await changesConfig(`${label} (the driver's own registration command)`, exec) : await exec();
    const rec = { label, argv: ['codex', ...argv], exitCode: r.exitCode ?? null, timedOut: !!r.timedOut, spawnError: r.spawnError ?? null, stdout: cap(r.stdout), stderr: cap(r.stderr) };
    commands.push(rec);
    return { rec, raw: r };
  };
  const ok = (r) => r.exitCode === 0 && !r.timedOut && !r.spawnError;
  const hand = `remove it by hand: codex mcp remove ${serverName}`;
  const unregisterOnce = async () => {
    try {
      const { rec: r } = await step('remove (teardown)', registrationArgv.remove(serverName), { change: true });
      const { rec: gone, raw } = await step('get (after teardown)', registrationArgv.get(serverName));
      const clean = !ok(gone) && gone.exitCode !== null && !gone.timedOut && !gone.spawnError && NOT_REGISTERED.test(`${raw.stdout ?? ''}\n${raw.stderr ?? ''}`);
      removed = clean;
      if (!removed) finding(`the MCP server ${serverName} is not shown to be removed from Codex after the teardown removal (removal exit ${r.exitCode ?? r.spawnError ?? 'timeout'}; the check after it ${gone.timedOut ? 'timed out' : gone.spawnError ? 'could not be run' : gone.exitCode === 0 ? 'still finds it' : 'did not say it was not found'}); ${hand}`);
      return r;
    } catch (err) {
      finding(`the teardown removal of the MCP server ${serverName} failed (${String(err?.message ?? err).slice(0, 120)}); ${hand}`);
      return null;
    }
  };
  return {
    commands,
    get attempted() {
      return attempted;
    },
    get removed() {
      return removed;
    },
    // Check what is there, remove a leftover of this driver's own (a finding), then add and verify.
    async register() {
      attempted = true;
      const { rec: before, raw } = await step('get (before)', registrationArgv.get(serverName));
      if (ok(before)) {
        const shape = ownEntryShape(raw.stdout, oac);
        if (!shape.own) {
          // Not ours: record nothing of it, touch nothing, and never remove it at teardown.
          before.stdout = '<not recorded: an entry of this name that is not this driver\'s own>';
          before.stderr = '';
          attempted = false;
          throw new RegistrationRefused(`an MCP server named ${serverName} is already registered with Codex and is not this driver's own entry (its command or arguments differ); the driver does not remove or replace it. Remove or rename it yourself, then re-run`);
        }
        finding(`an MCP server named ${serverName} was already registered with Codex before the run, with this driver's own command (a leftover of an earlier run); the driver removes it and registers its own`);
      }
      // Removal tolerates "not found": Codex answers it with exit 0 and a message.
      await step('remove (before)', registrationArgv.remove(serverName), { change: true });
      const { rec: add } = await step('add', registrationArgv.add(serverName, ns, oac), { change: true });
      if (!ok(add)) throw new Error(`codex mcp add ${serverName} failed (exit ${add.exitCode ?? add.spawnError ?? 'timeout'})`);
      const { rec: after } = await step('get (after)', registrationArgv.get(serverName));
      if (!ok(after)) throw new Error(`codex mcp get ${serverName} after the add failed (exit ${after.exitCode ?? after.spawnError ?? 'timeout'})`);
      return after;
    },
    unregister() {
      if (!attempted || removed) return Promise.resolve(null);
      inflight ??= unregisterOnce().finally(() => {
        inflight = null;
      });
      return inflight;
    },
  };
}
