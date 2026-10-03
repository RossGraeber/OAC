// g4-mcp-dual-era: G4 re-run end to end through the herdr driver (Epic K, K8 #131).
//
// NOT VERDICT-BEARING. This scenario replays the human-run G4 re-run of 2026-09-26
// (docs/planning/gates/G4-result.md) through herdr and records the run so it can be compared
// against that run's fixture, criterion by criterion (tools/herdr/lib/g4-report.mjs). It never
// changes G4's verdict, STATUS.md, or PINS.md.
//
// THE SERVER IS A RECONSTRUCTION. The G4 spike server was never committed; this scenario runs
// tools/herdr/gate-servers/g4-server.mjs, rebuilt from G4-result.md's architecture and the
// committed fixture's wire shapes. It is not the server that produced the human-run fixture
// and cannot be verified identical to it; every comparison says so.
//
// LIVE STATUS: UNVERIFIED. Exercised only against the test doubles in tools/herdr/test/ (a
// fake herdr, a fake Claude Code and a fake Codex); it has never driven a real herdr, Claude
// Code or Codex. Every pane-text pattern it schedules on (lib/g1.mjs, lib/g2.mjs) is a guess
// except the dev-channels dialog, and whether Codex honors a per-invocation MCP-server `-c`
// override for an HTTP server is itself UNVERIFIED.
//
// Operator command (herdr at the PINS.md pin; Claude Code and Codex at any version, since
// versions float and a difference from PINS.md's last tested versions is a VERSION WARNING
// finding, never a stop (#216); both signed in the way the operator normally uses them; ports 17458 and 17460
// free on 127.0.0.1):
//
//   node tools/herdr/run.mjs --scenario g4-mcp-dual-era \
//     --launch '["claude","--dangerously-load-development-channels","server:g4spike","server:g4modern"]' \
//     --out <run dir>
//   node tools/herdr/lib/g4-report.mjs --run <run dir>            # draft comparison
//
// What it does, in the human run's order:
//   0. Preflight. The Claude launch must be G4's, verbatim. The Codex launch (--param
//      codexLaunch, a JSON argv; default `codex -c mcp_servers.g4http.url="http://127.0.0.1:
//      <httpPort>/mcp"`) may carry only per-invocation `-c` overrides of MCP-server url /
//      enabled / timeout keys and feature flags, and only allowlisted values (#244: a
//      loopback http(s) URL without userinfo, query or fragment; a boolean; a number). A
//      --param codexLaunch that breaks those rules is refused by validateParams before the
//      driver creates or records anything (usage error, exit 2, no run manifest). The
//      operator's global Codex config is never
//      edited, no Codex config file is written, and the Codex home is never copied or
//      redirected. `claude --version` and `codex --version` are compared with PINS.md's
//      committed minimum and last tested versions; a difference is a VERSION WARNING
//      finding and the run continues (versions float, warn, never gate, #216). Both ports
//      must be free. The reconstructed server is staged into scratch; a project .mcp.json
//      registers it three ways, as the human run's `/mcp` list shows: g4spike (legacy stdio
//      channel), g4modern (G4_STDIO_MODERN=1, the negative case) and g4http (HTTP).
//   1. Claude launches through `herdr agent start --kind claude -- <launch[1..]>` with
//      MCP_SDK_GENERATION=v2 in its pane environment (the human run's setting; --param
//      claudeEnv). Every dialog is read verbatim before any keystroke; accept=driver (the
//      default since #196) accepts Claude Code's workspace-trust, MCP-server and dev-channels
//      dialogs by the verified key plan (lib/gate-common.mjs driverAcceptDialog) and records
//      the accept as `driver`; accept=human sends nothing. The dev-channels confirmation
//      scores nothing in G4.
//   2. The wire must show, from ONE server process: the legacy stdio `initialize`, the HTTP
//      surface listening, Claude's modern HTTP `server/discover`; and from the g4modern copy
//      a modern-only `server/discover`. The wire clientInfo.version must equal the CLI version.
//   3. Wake 1: `wake.trigger` is touched (the server's own trigger; herdr types nothing),
//      both copies push; the Claude pane is read.
//   4. Claude's modern HTTP `tools/call` (an operator prompt).
//   5. Codex starts in a second pane, CONCURRENTLY with the live Claude session. Its MCP
//      client's `initialize` user-agent must equal the pinned Codex version. An operator
//      prompt asks it to call g4_echo, then g4_relay_to_claude; the relay reaches Claude only
//      as the server's own channel push.
//   6. Claude's modern `tools/call` again, after the Codex traffic (no-degradation check).
//   7. Wake 2.
//   8. Post-run versions.
//   Captures (sanitized by lib/g4.mjs, then redacted by run.mjs):
//   `transcript-<date>-claude-<v>-codex-<v>-herdr.jsonl` (the server's own transcript, every
//   copy), `pane-claude-<date>-<v>-herdr.txt`, `pane-codex-<date>-<v>-herdr.txt`.
//
// Credentials: nothing here reads a file under either harness's home or asks any OS credential
// store for anything. tools/herdr/test/g4-tests.mjs traces every file the driver opens during a
// fake run and fails if it touches the Codex home beyond run.mjs's config hashes.

import { existsSync, readFileSync, utimesSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { DriverError } from '../lib/herdr.mjs';
import { parseClaudeVersions, pinsReadWarning, parseClaudeCliVersion, claudeVersionWarning, parseCodexVersions, parseCodexCliVersion, codexVersionWarning, CLAUDE_PIN_ROW, CODEX_PIN_ROW } from '../lib/pins.mjs';
import { harnessVersions } from '../lib/manifest.mjs';
import { committedFile, classifyScreen, driverMayAccept, DIALOG_KINDS } from '../lib/g1.mjs';
import { classifyCodexScreen, driverMayAcceptCodex, CODEX_DIALOG_KINDS, paneArgv } from '../lib/g2.mjs';
import { descendants } from '../lib/proc.mjs';
import { makeAgent, stopper, stageGateFiles, loopbackPortFree } from '../lib/gate-common.mjs';
import {
  G4_LAUNCH, G4_SERVER_FILES, PINS_PATH, DEFAULT_PORTS, DEFAULT_PROMPTS, g4McpJson, defaultCodexLaunch, validateCodexLaunch, codexLaunchParamProblem, validatePaneEnv, assertNotInjected,
  fixtureNames, unverifiedNames, parseG4Transcript, g4Facts, roles, sanitizeG4Transcript, sanitizeG4Text, MODERN, LEGACY, HUMAN_RUN_PORTS, codexSessions,
} from '../lib/g4.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..', '..');
const sleep = (ms) => new Promise((r) => setTimeout(r, Math.max(0, ms)));

export default {
  name: 'g4-mcp-dual-era',
  description: 'G4 re-run through herdr for comparison with the human-run 2026-09-26 re-run (K8). Reconstructed server. Not verdict-bearing.',
  harnesses: ['claude', 'codex'],
  // #244: run.mjs calls this before it creates scratch, an output directory or a manifest, so a
  // refused codexLaunch leaves no record at all. The reason names no argument text.
  validateParams: ({ params }) => codexLaunchParamProblem(params),
  defaults: {
    launch: [...G4_LAUNCH],
    timeboxMs: 60 * 60 * 1000, // the 2026-09-26 re-run declared 60 minutes (G4-result.md "Timebox")
    params: {
      accept: 'driver', // #196: the driver accepts recognized dialogs by default; accept=human remains
      httpPort: String(DEFAULT_PORTS.httpPort),
      modernHttpPort: String(DEFAULT_PORTS.modernHttpPort),
      codexLaunch: '',
      claudeEnv: JSON.stringify({ MCP_SDK_GENERATION: 'v2' }),
      claudeEchoPrompt: DEFAULT_PROMPTS.claudeEchoPrompt,
      codexToolsPrompt: DEFAULT_PROMPTS.codexToolsPrompt,
      claudeEchoAfterPrompt: DEFAULT_PROMPTS.claudeEchoAfterPrompt,
      startupTimeoutMs: '120000',
      humanAcceptTimeoutMs: '300000',
      handshakeTimeoutMs: '90000',
      wireTimeoutMs: '15000',
      turnTimeoutMs: '300000',
      settleMs: '3000',
      pollMs: '1000',
      busyIndicator: 'esc to interrupt',
      readLines: '2000',
      maxDialogs: '12',
    },
  },

  async run(ctx) {
    const { herdr, launch, params } = ctx;
    const num = (k) => {
      const v = Number(params[k]);
      if (!(v >= 0)) throw new DriverError(`--param ${k} must be a non-negative number`);
      return v;
    };
    const accept = params.accept;
    if (!['human', 'driver'].includes(accept)) throw new DriverError('--param accept must be human or driver');
    const httpPort = num('httpPort');
    const modernHttpPort = num('modernHttpPort');
    let codexLaunch;
    let claudeEnv;
    try {
      codexLaunch = params.codexLaunch ? JSON.parse(params.codexLaunch) : defaultCodexLaunch(httpPort);
      claudeEnv = JSON.parse(params.claudeEnv || '{}');
    } catch {
      throw new DriverError('--param codexLaunch must be a JSON argv and claudeEnv a JSON object');
    }
    const prompts = { claudeEchoPrompt: params.claudeEchoPrompt, codexToolsPrompt: params.codexToolsPrompt, claudeEchoAfterPrompt: params.claudeEchoAfterPrompt };
    try {
      for (const [k, v] of Object.entries(prompts)) assertNotInjected(k, v);
    } catch (err) {
      throw new DriverError(err.message);
    }

    const g4 = {
      nonVerdictBearing: 'K8: compared against the human-run G4 2026-09-26 re-run; never changes the G4 verdict',
      reconstruction: 'tools/herdr/gate-servers/g4-server.mjs is REBUILT from G4-result.md and the committed fixture; the original spike server was never committed, so this is not the server that produced the baseline',
      acceptPolicy: accept,
      params: { ...params, ...prompts },
      launch: { expected: [...G4_LAUNCH], actual: launch, verbatim: null },
      codexLaunch: { argv: codexLaunch, validation: null, herdrReportedArgv: null, paneArgv: null, globalConfigEdited: false, codexHomeCopied: false },
      claudeEnv,
      ports: { httpPort, modernHttpPort, freeBefore: null },
      versions: null,
      server: null,
      mcpJson: null,
      date: new Date().toISOString().slice(0, 10),
      fixtures: null,
      captureNames: null,
      dialogs: [],
      herdrStates: [],
      handshake: null,
      triggers: [],
      wakes: [],
      claudeEcho: null,
      codex: null,
      claudeEchoAfter: null,
      postRun: null,
      sanitizer: null,
      stoppedAt: null,
    };
    const stop = stopper(herdr, g4);
    let serverDir = null;

    const claude = makeAgent({ ctx, g: g4, name: 'g4claude', label: 'claude', classify: (t) => classifyScreen(t, { busyIndicator: params.busyIndicator }), dialogKinds: DIALOG_KINDS, driverMayAccept, accept, num, stop });
    const codex = makeAgent({ ctx, g: g4, name: 'g4codex', label: 'codex', classify: (t) => classifyCodexScreen(t, { busyIndicator: params.busyIndicator }), dialogKinds: CODEX_DIALOG_KINDS, driverMayAccept: driverMayAcceptCodex, accept, num, stop });

    const transcriptPath = () => join(serverDir, 'transcript.jsonl');
    const facts = () => g4Facts(serverDir && existsSync(transcriptPath()) ? parseG4Transcript(readFileSync(transcriptPath(), 'utf8'), { completeLinesOnly: true }) : []);
    const fire = () => {
      const p = join(serverDir, 'wake.trigger');
      const now = new Date();
      if (existsSync(p)) utimesSync(p, now, now);
      else writeFileSync(p, '');
      const rec = { at: now.toISOString() };
      g4.triggers.push(rec);
      return rec;
    };
    const prompt = async (agent, label, text) => {
      assertNotInjected(label, text);
      return agent.prompt(text);
    };

    try {
      // --- 0. preflight -----------------------------------------------------------------------
      g4.launch.verbatim = JSON.stringify(launch) === JSON.stringify(G4_LAUNCH);
      if (!g4.launch.verbatim) throw new DriverError(`launch ${JSON.stringify(launch)} is not G4's verbatim launch ${JSON.stringify(G4_LAUNCH)}; this would not be a G4 re-run`);
      const v = validateCodexLaunch(codexLaunch);
      g4.codexLaunch.validation = v;
      if (!v.ok) throw new DriverError(`codexLaunch refused: ${v.why}. Codex's MCP registration must be per invocation; the operator's global config is never edited`);
      const envProblem = validatePaneEnv(claudeEnv);
      if (envProblem) throw new DriverError(`claudeEnv refused: ${envProblem}`);

      const pinsFile = committedFile(REPO, PINS_PATH);
      const pinsText = pinsFile.bytes.toString('utf8');
      // Versions float and are never gated (#216): every difference from PINS.md's minimum
      // or last tested version is a VERSION WARNING finding, and the run continues.
      const cpin = parseClaudeVersions(pinsText);
      const xpin = parseCodexVersions(pinsText);
      const cRaw = ctx.harnessVersion('claude');
      const xRaw = ctx.harnessVersion('codex');
      const cli = { claude: parseClaudeCliVersion(cRaw), codex: parseCodexCliVersion(xRaw) };
      g4.versions = {
        pins: { claudeRow: CLAUDE_PIN_ROW, claudeMinimum: cpin.minimum, claudeLastTested: cpin.lastTested, codexRow: CODEX_PIN_ROW, codexMinimum: xpin.minimum, codexLastTested: xpin.lastTested, codexCommit: xpin.commit, headCommit: pinsFile.headCommit, workingTreeMatchesHead: pinsFile.workingTreeMatchesHead },
        cliOutput: { claude: cRaw, codex: xRaw },
        cli,
        wire: { claude: null, codex: null },
        verified: false, // true once each harness's CLI and wire report one and the same version
        matchesLastTested: null,
        warnings: [],
      };
      const warn = (w) => {
        if (!w) return;
        g4.versions.warnings.push(w);
        ctx.finding(w);
      };
      if (!pinsFile.workingTreeMatchesHead) ctx.finding(`${PINS_PATH} has uncommitted changes; the version checks read the committed PINS.md (HEAD ${pinsFile.headCommit})`);
      warn(pinsReadWarning(cpin, 'G4'));
      warn(pinsReadWarning(xpin, 'G4'));
      for (const [h, raw] of [['claude', cRaw], ['codex', xRaw]]) if (!cli[h] && /^N\/A/.test(raw ?? 'N/A')) stop(`${h} --version could not be run (${raw ?? 'not recorded'}); nothing launched`);
      warn(claudeVersionWarning({ observed: cli.claude, lastTested: cpin.lastTested, minimum: cpin.minimum, source: '`claude --version`', gate: 'G4' }));
      warn(codexVersionWarning({ observed: cli.codex, lastTested: xpin.lastTested, minimum: xpin.minimum, source: '`codex --version`', gate: 'G4' }));
      g4.captureNames = unverifiedNames(g4.date);

      for (const p of [httpPort, modernHttpPort]) {
        if (HUMAN_RUN_PORTS.includes(p)) throw new DriverError(`port ${p} is the human G4 run's; a leftover global Codex registration from that run may still point at it, and its traffic could not be told apart from this run's per-invocation registration. Use another port`);
      }
      console.error(
        `\n[g4-mcp-dual-era] Before relying on this run, check \`codex mcp list\` (read-only) yourself: any Codex MCP entry pointing at 127.0.0.1:${httpPort} besides this run's per-invocation \`g4http\` would connect too.\n` +
          '  The driver never reads or edits your Codex config; the report flags more than one Codex session on the wire.\n',
      );
      g4.ports.freeBefore = { [httpPort]: await loopbackPortFree(httpPort), [modernHttpPort]: await loopbackPortFree(modernHttpPort) };
      for (const p of [httpPort, modernHttpPort]) if (g4.ports.freeBefore[p] !== true) stop(`127.0.0.1:${p} is not free (a stale G4 server or another listener holds it); nothing launched`);

      serverDir = ctx.dir('g4-server');
      g4.server = stageGateFiles(REPO, G4_SERVER_FILES, serverDir);
      if (!g4.server.every((s) => s.match)) throw new DriverError('a staged gate-server copy does not match its working-tree source (sha256)');
      const projectDir = ctx.dir('g4-project');
      const mcp = g4McpJson({ node: process.execPath, serverPath: g4.server[0].copy, httpPort, modernHttpPort });
      writeFileSync(join(projectDir, '.mcp.json'), `${JSON.stringify(mcp, null, 2)}\n`);
      g4.mcpJson = { path: join(projectDir, '.mcp.json'), contents: mcp };

      // --- 1. Claude launch; dialogs ----------------------------------------------------------
      const ws = await herdr.workspaceCreate({ cwd: projectDir, label: 'oac-g4-claude', env: claudeEnv });
      ctx.record('workspace', ws);
      await herdr.paneProcessInfo(ws.paneId);
      await ctx.probeEnv(ws.paneId);
      const started = await ctx.startAgent('g4claude', { paneId: ws.paneId, timeoutMs: num('startupTimeoutMs'), allowErrorCodes: ['agent_not_ready'] });
      g4.claudeStart = { seq: herdr.commands.at(-1).seq, errorCode: started.errorCode, herdrReportedArgv: started.argv };
      await herdr.paneProcessInfo(ws.paneId);
      await claude.settle('startup', num('startupTimeoutMs'));

      // --- 2. handshake, both eras, one process ------------------------------------------------
      const hs = await claude.waitFor('the dual-era handshake on the wire', () => {
        const f = facts();
        const r = roles(f);
        if (!r.legacyPid || !r.modernPid) return null;
        const init = f.stdioInitialize.find((x) => x.pid === r.legacyPid && x.resLine);
        const listed = f.stdioToolsList.find((x) => x.pid === r.legacyPid && x.resLine);
        const hd = f.httpDiscover.find((x) => x.pid === r.legacyPid && x.resLine && !x.error);
        const md = f.stdioDiscover.find((x) => x.pid === r.modernPid && x.resLine && !x.error);
        return init && listed && hd && md ? { f, r, init, hd, md } : null;
      }, num('handshakeTimeoutMs'), { lbl: 'handshake-wait' });
      if (hs.f.listenErrors.some((e) => e.pid === hs.r.legacyPid)) stop('the legacy server copy could not listen on its HTTP port; nothing delivered');
      g4.handshake = {
        legacyPid: hs.r.legacyPid,
        modernPid: hs.r.modernPid,
        legacyInitialize: hs.init,
        httpDiscover: hs.hd,
        modernDiscover: hs.md,
        instances: hs.f.instances,
      };
      g4.versions.wire.claude = hs.init.clientInfo?.version ?? null;
      warn(claudeVersionWarning({ observed: /^\d+\.\d+\.\d+$/.test(String(g4.versions.wire.claude ?? '')) ? g4.versions.wire.claude : null, lastTested: cpin.lastTested, minimum: cpin.minimum, source: 'the wire initialize clientInfo.version', gate: 'G4' }));
      // #246/#253: every full read after a turn is a settled read (gate-common settledRead).
      g4.afterStartupReadSeq = (await claude.settledRead('after-startup', { source: 'recent-unwrapped', lines: num('readLines') }, { context: 'post-handshake', timeoutMs: num('startupTimeoutMs') })).seq;

      const wake = async (n) => {
        const pre = await claude.read(`pre-wake-${n}-idle-check`);
        if (pre.screen.busy || pre.screen.dialog) stop(`the Claude session was not visibly idle before wake ${n}`);
        const before = facts().pushes.length;
        const trig = fire();
        const pushes = await claude.waitFor(`wake ${n} on the wire (both copies)`, () => {
          const p = facts().pushes.slice(before).filter((x) => x.meta.g4_stdio_era);
          return p.some((x) => x.pid === g4.handshake.legacyPid) && p.some((x) => x.pid === g4.handshake.modernPid) ? p : null;
        }, num('wireTimeoutMs'), { lbl: `wake-${n}-wait` });
        await sleep(num('settleMs'));
        const r = await claude.settledRead(`after-wake-${n}`, { source: 'recent-unwrapped', lines: num('readLines') }, { context: `wake-${n}-turn`, timeoutMs: num('turnTimeoutMs') });
        g4.wakes.push({ n, trigger: trig, preReadSeq: pre.seq, pushes: pushes.map((x) => ({ pid: x.pid, era: x.era, line: x.line, t: x.t, id: x.meta.oac_message_id })), afterReadSeq: r.seq });
      };
      const claudeEcho = async (key, label, text) => {
        const before = facts().toolCalls.length;
        const p = await prompt(claude, label, text);
        const call = await claude.waitFor(`Claude's modern tools/call (${label})`, () => facts().toolCalls.slice(before).find((c) => c.era === 'modern' && c.name === 'g4_echo' && c.resLine) ?? null, num('turnTimeoutMs'), { lbl: `${label}-wait` });
        // #253: the prompt's own turn (since); the tools/call on the wire shows it began.
        const r = await claude.settledRead(`after-${label}`, { source: 'recent-unwrapped', lines: num('readLines') }, { context: label, timeoutMs: num('turnTimeoutMs'), since: p, done: () => true });
        g4[key] = { prompt: p, call: { reqLine: call.reqLine, resLine: call.resLine, pid: call.pid, text: call.text }, afterReadSeq: r.seq };
      };

      // --- 3. wake 1 -----------------------------------------------------------------------------
      await wake(1);
      // --- 4. Claude modern tools/call ----------------------------------------------------------
      await claudeEcho('claudeEcho', 'claude-echo', prompts.claudeEchoPrompt);

      // --- 5. Codex, concurrently, per-invocation registration -------------------------------------
      const cws = await herdr.workspaceCreate({ cwd: ctx.dir('g4-codex-project'), label: 'oac-g4-codex' });
      ctx.record('codexWorkspace', cws);
      await herdr.paneProcessInfo(cws.paneId);
      const httpBefore = facts().httpInitialize.length;
      const cstart = await herdr.agentStart('g4codex', { launchArgv: codexLaunch, paneId: cws.paneId, timeoutMs: num('startupTimeoutMs'), allowErrorCodes: ['agent_not_ready'] });
      g4.codexLaunch.herdrReportedArgv = cstart.argv;
      // One process table for the pane's whole tree, the one the driver took for its query
      // (#136 review, #244 note D: each table is one WMI query, ~1.6 s, on Windows).
      const { info, table: procTable } = await herdr.paneProcessSnapshot(cws.paneId);
      const fg = (info.foreground_processes ?? []).map((p) => p.pid).filter(Number.isInteger);
      const tree = [...new Set([...fg, ...fg.flatMap((p) => descendants(p, procTable) ?? []), ...(descendants(info.shell_pid, procTable) ?? [])])];
      // #232: argv minimized; the only arguments kept verbatim are the validated per-invocation
      // overrides this launch asserts, compared on the full argv in memory.
      const { argv: argvRecords, proof } = paneArgv(tree, procTable, { allow: codexLaunch.slice(1), expectArgsAfterCodex: codexLaunch.slice(1) });
      g4.codexLaunch.paneArgv = { argv: argvRecords, proof, matchesLaunch: proof.found ? proof.matchesExpected : null };
      if (proof.found && !g4.codexLaunch.paneArgv.matchesLaunch) throw new DriverError(`the pane's codex process runs with ${JSON.stringify(proof.argsAfterCodex)}, not the validated per-invocation overrides ${JSON.stringify(codexLaunch.slice(1))}`);
      if (!proof.found) ctx.finding('the Codex pane\'s process argv could not show a `codex` process; the per-invocation launch rests on the launch parameter and herdr\'s reported argv only');
      await codex.settle('codex-startup', num('startupTimeoutMs'));
      const cinit = await codex.waitFor('Codex\'s MCP client initialize on the HTTP surface', () => facts().httpInitialize.slice(httpBefore).find((x) => x.resLine && x.codexVersion) ?? null, num('handshakeTimeoutMs'), { lbl: 'codex-mcp-wait' });
      g4.versions.wire.codex = cinit.codexVersion;
      warn(codexVersionWarning({ observed: cinit.codexVersion, lastTested: xpin.lastTested, minimum: xpin.minimum, source: 'the MCP client user-agent (codex-mcp-client/<version>)', gate: 'G4' }));
      if (cli.claude && cli.codex && g4.versions.wire.claude === cli.claude && g4.versions.wire.codex === cli.codex) {
        g4.versions.verified = true; // each harness's CLI and wire report one and the same version
        g4.versions.matchesLastTested = cli.claude === cpin.lastTested && cli.codex === xpin.lastTested;
        g4.fixtures = fixtureNames(g4.date, cli.claude, cli.codex);
        g4.captureNames = g4.fixtures;
      } else {
        ctx.finding(`a harness's CLI and wire versions differ (CLI ${JSON.stringify(g4.versions.cliOutput)}, wire ${JSON.stringify(g4.versions.wire)}); the run continues, but its captures stay unverified-* because they cannot name one version per harness`);
      }

      const callsBefore = facts().toolCalls.length;
      const pushesBefore = facts().pushes.length;
      const cp = await prompt(codex, 'codexToolsPrompt', prompts.codexToolsPrompt);
      const codexCalls = await codex.waitFor('Codex\'s g4_echo and g4_relay_to_claude calls and the relay push', () => {
        const f = facts();
        const calls = f.toolCalls.slice(callsBefore).filter((c) => c.codexVersion && c.resLine);
        const echo = calls.find((c) => c.name === 'g4_echo');
        const relay = calls.find((c) => c.name === 'g4_relay_to_claude');
        const push = f.pushes.slice(pushesBefore).find((x) => x.meta.relay_from);
        return echo && relay && push ? { echo, relay, push } : null;
      }, num('turnTimeoutMs'), { lbl: 'codex-tools-wait' });
      const cr = await codex.settledRead('after-codex-tools', { source: 'recent-unwrapped', lines: num('readLines') }, { context: 'codex-tools', timeoutMs: num('turnTimeoutMs'), since: cp, done: () => true });
      const rr = await claude.settledRead('after-relay', { source: 'recent-unwrapped', lines: num('readLines') }, { context: 'relay-turn', timeoutMs: num('turnTimeoutMs') });
      const sessions = codexSessions(facts(), g4.handshake.legacyPid);
      if (sessions.length !== 1) ctx.finding(`${sessions.length} Codex HTTP MCP sessions reached the server (initialize at lines ${sessions.map((x) => x.reqLine).join(', ')}); only one per-invocation registration was passed, so another Codex registration (for example a leftover entry in the operator's own Codex config) also connected, and the Codex traffic cannot be attributed to the per-invocation registration alone`);
      g4.codex = {
        sessions: sessions.map((x) => ({ reqLine: x.reqLine, sessionId: x.sessionId })),
        prompt: cp,
        mcpInitialize: { reqLine: cinit.reqLine, resLine: cinit.resLine, requested: cinit.requested, negotiated: cinit.negotiated, userAgent: cinit.userAgent },
        echo: { reqLine: codexCalls.echo.reqLine, resLine: codexCalls.echo.resLine, era: codexCalls.echo.era },
        relay: { reqLine: codexCalls.relay.reqLine, resLine: codexCalls.relay.resLine, era: codexCalls.relay.era },
        relayPush: { line: codexCalls.push.line, pid: codexCalls.push.pid, id: codexCalls.push.meta.oac_message_id },
        afterReadSeq: cr.seq,
        claudeAfterRelayReadSeq: rr.seq,
      };

      // --- 6. Claude modern tools/call after the Codex traffic ------------------------------------
      await claudeEcho('claudeEchoAfter', 'claude-echo-after', prompts.claudeEchoAfterPrompt);
      // --- 7. wake 2 -----------------------------------------------------------------------------
      await wake(2);

      // --- 8. post-run versions --------------------------------------------------------------------
      const post = await harnessVersions(['claude', 'codex']);
      g4.postRun = { cliOutput: post, claude: parseClaudeCliVersion(post.claude), codex: parseCodexCliVersion(post.codex) };
      g4.postRun.matches = g4.postRun.claude === cli.claude && g4.postRun.codex === cli.codex;
      if (!g4.postRun.matches) {
        ctx.finding(`a harness version changed during the run (${JSON.stringify(g4.versions.cliOutput)} before, ${JSON.stringify(post)} after); the captures lose their fixture names`);
        g4.versions.verified = false;
        g4.fixtures = null;
        g4.captureNames = unverifiedNames(g4.date);
      }
      const f = facts();
      g4.summary = { legacyPid: g4.handshake.legacyPid, modernPid: g4.handshake.modernPid, era: { claudeHttp: MODERN, legacyStdio: LEGACY }, codexEras: [...new Set(f.httpInitialize.filter((x) => x.codexVersion).map((x) => x.negotiated))], httpSessions: f.httpInitialize.length };
      await herdr.paneProcessInfo(ws.paneId);
    } finally {
      ctx.record('g4', g4);
      let transcriptText = null;
      if (serverDir && existsSync(transcriptPath())) {
        const s = sanitizeG4Transcript(readFileSync(transcriptPath(), 'utf8'));
        g4.sanitizer = s.report;
        transcriptText = s.text;
      }
      // The pane captures get the same substitution: the prompts ask both harnesses to print
      // tool results, which can show the identifier on screen.
      const paneClaude = sanitizeG4Text(claude.sections.join(''));
      const paneCodex = sanitizeG4Text(codex.sections.join(''));
      g4.sanitizer = { ...(g4.sanitizer ?? { extensionIdReplaced: 0 }), paneClaude: paneClaude.replaced, paneCodex: paneCodex.replaced };
      if (g4.captureNames) {
        if (claude.sections.length) ctx.capture(g4.captureNames.paneClaude, paneClaude.text);
        if (codex.sections.length) ctx.capture(g4.captureNames.paneCodex, paneCodex.text);
        if (transcriptText !== null) ctx.capture(g4.captureNames.transcript, transcriptText, { format: 'jsonl' });
      }
    }
  },
};

