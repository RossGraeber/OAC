// g5-claude-resume: G5 (#66) box 4, the live check of registration across start, exit, --resume and
// --continue. NOT VERDICT-BEARING (G5 is not a gate).
//
// A real Claude Code (print mode, so no dialog) is started by herdr three times in one working
// directory, each time with the OAC shim as its MCP server and the OAC hook handler on SessionStart,
// against the example live daemon (cli/examples/live_daemon.rs: an in-memory key, the in-memory
// transport and a test namespace, so nothing of the user's device key is touched):
//   run A: a fresh session;
//   run B: `claude -p --resume <run A's Claude session id>`;
//   run C: `claude -p --continue`.
// Each run is held alive by one allowed Bash call (`sleep N`) so that `oac sessions --json` (in the
// test namespace) can list it. It shows, on the machine it runs on:
//   - registration: each run is listed as one bound session (harness `claude`, state `online`, no
//     unbound attachment) while it is alive;
//   - deregistration: after each run exits, no session is listed (SC-ID-155);
//   - the C4 section 6 rule, as the fake-harness test registration.rs asserts it: a resumed or
//     continued Claude session is a NEW OAC session (its OAC session id differs from every earlier one);
//   - what Claude Code itself reported as its session id for each run (whether --resume and --continue
//     kept run A's), from its `--output-format json` result. The value is recorded, not judged (OAC does not
//     depend on it); that each run produced a result with a session id is judged;
//   - the daemon's own count (six connections accepted and six ended: a shim and a hook handler per
//     run), its binding log (no `Finding`), and, per run, the SC-ID-142 diagnostics. Those diagnostics
//     are recorded and never fail the run.
//
// How the runs are told apart and sampled (#472):
//   - One Claude pane serves all three runs. Each run's command line starts with an `echo` of a marker
//     that is unique to the run, and everything the scenario reads from the pane (the result JSON, the
//     `missing operand` check, the capture) is the text from that command line on, so a run with no
//     result of its own cannot inherit an earlier run's `session_id`.
//   - The session list is sampled while each run is alive, at least twice, on events of the daemon's own
//     output: after its first `accepted` event of the run, and after the first `freed a connection` log
//     line of the run (the hook handler's connection). That line is NOT a bind signal: `cli/src/daemon.rs`
//     writes it after `disconnect` returned, and in the committed events of run C the bind decision comes
//     after it. The daemon writes no event for an ordinary bind (only for the SC-ID diagnostics), so when the
//     hook-freed sample lists nothing and the run is still alive, up to `MAX_BIND_RETRIES` more samples are
//     taken, each started by the completion of the previous sample's command (a round trip through the ops
//     pane, not a timer), stopping at the first alive sample that lists a session. That sample is the one the
//     bound-session criteria are judged on. A last sample follows the shim's `freed` line, after the run
//     exited. No timer is involved: every wait is a herdr wait-output (`recent-unwrapped`, so a long line
//     is not cut by the pane width) on an event line whose `unix_ms` is later than the daemon's last event
//     before the run started.
//   - Every session id and Claude session id is registered for redaction the moment it is first seen,
//     and each `sessions-*` capture is written at its own step, so a run that fails keeps its evidence.
//
// No project files are written (the MCP server and the hook come from --mcp-config / --strict-mcp-config
// and --settings files in scratch), so Claude Code shows no trust or MCP-approval dialog; no credential is
// read and the harness config is not edited. The scenario accepts no dialog: print mode shows none.
//
// Operator command:
//   node tools/herdr/run.mjs --scenario g5-claude-resume --param oac=<abs path to oac(.exe)> \
//     --param daemon=<abs path to the live_daemon example> [--param namespace=g5r] \
//     [--param holdSeconds=30] --out <run dir>

import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { randomBytes } from 'node:crypto';

import { regexLiteral } from '../lib/pane-shell.mjs';

// Three runs, a shim and a hook handler each.
export const RUNS = 3;
export const EXPECTED_CONNECTIONS = 2 * RUNS;
// Extra session-list samples after the hook-freed one while a run is alive and nothing is listed yet.
export const MAX_BIND_RETRIES = 5;

// An OAC session id (core/src/ids.rs is_session_id): 26 characters, the first 0-7, the rest lower-case Crockford base32.
export const OAC_ID_SHAPE = /(?<![0-9a-z])[0-7][0-9a-hjkmnp-tv-z]{25}(?![0-9a-z])/g;
export const oacIdsIn = (text) => [...text.matchAll(OAC_ID_SHAPE)].map((m) => m[0]);

export function jsonLines(text) {
  const out = [];
  for (const line of text.split(/\r?\n/)) {
    const t = line.trim();
    if (!t.startsWith('{')) continue;
    try {
      out.push(JSON.parse(t));
    } catch {
      /* a partial line */
    }
  }
  return out;
}

// A regex (herdr's Rust syntax and JS's alike) for a positive integer written in decimal that is greater than
// `t`: for each digit of `t` below 9, the same prefix, a greater digit, then any digits; or a longer number.
export function unixMsAfterPattern(t) {
  const s = String(t);
  if (!/^[1-9]\d*$/.test(s)) throw new Error('unixMsAfterPattern needs a positive integer');
  const alts = [];
  for (let i = 0; i < s.length; i += 1) {
    const d = Number(s[i]);
    if (d === 9) continue;
    const rest = s.length - 1 - i;
    alts.push(`${s.slice(0, i)}${d === 8 ? '9' : `[${d + 1}-9]`}${rest ? `\\d{${rest}}` : ''}`);
  }
  alts.push(`[1-9]\\d{${s.length},}`);
  return alts.join('|');
}

// The daemon's event line of kind `event` (live_daemon.rs `emit`: {"event":"x","unix_ms":N,...}) later than `t`.
export function eventAfterRegex(event, t) {
  return `"event":"${event}","unix_ms":(?:${unixMsAfterPattern(t)})[,}]`;
}

// The daemon's log line "freed a connection on claude" (cli/src/daemon.rs `connection_freed`), later than `t`.
export function freedAfterRegex(t) {
  return `"event":"log","unix_ms":(?:${unixMsAfterPattern(t)}),"line":"${regexLiteral('oac-daemon: freed a connection on claude')}"`;
}

const isFreed = (e) => e.event === 'log' && typeof e.line === 'string' && /\bfreed a connection on claude\b/.test(e.line);

// Events (objects with a string `event` and a numeric `unix_ms`) read from pane or file text.
export function daemonEvents(text) {
  return jsonLines(text).filter((e) => e && typeof e.event === 'string' && Number.isFinite(e.unix_ms));
}

// The daemon's clock at its last event in `text`, or null: the point after which "later" is this run's.
export function daemonClockMs(text) {
  const ms = daemonEvents(text).map((e) => e.unix_ms);
  return ms.length ? Math.max(...ms) : null;
}

// The earliest "freed a connection" log time later than `t`, or null.
export function firstFreedAfter(text, t) {
  const ms = daemonEvents(text).filter((e) => isFreed(e) && e.unix_ms > t).map((e) => e.unix_ms);
  return ms.length ? Math.min(...ms) : null;
}

// The pane text from the first line that holds `token` (the run's own command line) to the end; null if absent.
export function sliceFromToken(text, token) {
  const lines = text.split(/\r?\n/);
  const i = lines.findIndex((l) => l.includes(token));
  return i < 0 ? null : lines.slice(i).join('\n');
}

export const hasLine = (text, token) => text.split(/\r?\n/).some((l) => l.trim() === token);

// The Claude Code results (`--output-format json`) in a run's own pane text.
export function claudeResults(text) {
  return jsonLines(text).filter((x) => x && x.type === 'result' && typeof x.session_id === 'string');
}

// Every `"session_id":"..."` value in a text, whatever the line it is on.
export function sessionIdsIn(text) {
  return [...text.matchAll(/"session_id"\s*:\s*"([^"]+)"/g)].map((m) => m[1]);
}

// A sample taken while the run was alive that lists at least one session: the one the bound-session criteria use.
export const isBoundSample = (s) => Boolean(s.alive && s.code === 0 && s.o && s.o.sessions_total >= 1);

// What one run showed, from its samples. `samples` are the session lists taken while it was alive, in order; the
// bound sample is the first alive one that lists a session; `after` is the list after it exited.
export function summarizeRun(run) {
  const samples = run.samples ?? [];
  const alive = samples.filter((s) => s.alive);
  const bound = samples.find(isBoundSample) ?? null;
  const listed = bound?.o?.sessions ?? [];
  const sum = (o) => (Array.isArray(o?.unbound) ? o.unbound.reduce((n, u) => n + (Number.isInteger(u?.unbound_attachments) ? u.unbound_attachments : 1), 0) : null);
  const idsAlive = [...new Set(alive.flatMap((s) => (s.o?.sessions ?? []).map((x) => x?.session_id)))];
  // Once a session was listed, every later sample taken while the run was alive must list that session and nothing
  // else, with no unbound attachment: a session that is dropped again, or an attachment left over, is not "registered".
  const boundId = bound && listed.length === 1 && bound.o.sessions_total === 1 ? listed[0].session_id : null;
  const afterBound = bound ? samples.slice(samples.indexOf(bound) + 1).filter((s) => s.alive) : [];
  const unsteady = afterBound.filter((s) => !(s.code === 0 && s.o && s.o.sessions_total === 1 && s.o.sessions?.length === 1 && s.o.sessions[0]?.session_id === boundId && sum(s.o) === 0)).length;
  return {
    unsteadyAfterBound: unsteady,
    label: run.label,
    samples: samples.map((s) => ({ n: s.n, trigger: s.trigger, alive: s.alive, total: s.o?.sessions_total ?? null, unbound: sum(s.o) })),
    aliveSamples: alive.length,
    listedDuring: bound ? bound.o.sessions_total : 0,
    bound: Boolean(bound),
    oacSessionId: listed.length === 1 && bound?.o?.sessions_total === 1 ? listed[0].session_id : null,
    harness: listed[0]?.harness ?? null,
    state: listed[0]?.state ?? null,
    unboundDuring: sum(bound?.o),
    idsAlive,
    totalAfter: run.after?.o?.sessions_total ?? null,
    unboundAfter: sum(run.after?.o),
    afterExit: run.after?.code ?? null,
    claudeSessionId: run.claudeSessionId ?? null,
    sleepFailed: Boolean(run.sleepFailed),
  };
}

// The part of a run's summary that goes into the run manifest: counts and states, never an id.
export function observedRun(sum, runAId) {
  return {
    label: sum.label,
    listedDuring: sum.listedDuring,
    bound: sum.bound && Boolean(sum.oacSessionId),
    harness: sum.harness,
    state: sum.state,
    unboundDuring: sum.unboundDuring,
    totalAfter: sum.totalAfter,
    unboundAfter: sum.unboundAfter,
    aliveSamples: sum.aliveSamples,
    samples: sum.samples,
    sleepFailed: sum.sleepFailed,
    claudeSessionIsRunA: sum.claudeSessionId === null || runAId == null ? null : sum.claudeSessionId === runAId,
  };
}

// The stricter pass criteria (#472). Returns the list of unmet ones, as finding texts; empty means the leg passed.
// The SC-ID-142 diagnostics are not judged, and neither is the value of Claude Code's own session ids (that a result
// with a session id is present, is).
export function judgeG5(runs, daemon) {
  const problems = [];
  const sums = runs.map(summarizeRun);
  for (const r of sums) {
    const L = `run ${r.label}`;
    if (r.sleepFailed) problems.push(`${L}: the held Bash call failed; the session was not held`);
    if (r.aliveSamples < 2) problems.push(`${L}: ${r.aliveSamples} of ${r.samples.length} session-list sample(s) were taken while it was alive (at least 2 are required)`);
    if (r.listedDuring !== 1 || !r.oacSessionId) problems.push(`${L}: oac sessions did not list exactly one bound session while it was alive (listed ${r.listedDuring})`);
    else {
      if (r.harness !== 'claude' || r.state !== 'online') problems.push(`${L}: the bound session reported harness ${JSON.stringify(r.harness)} and state ${JSON.stringify(r.state)} (expected "claude" and "online")`);
      if (r.unboundDuring !== 0) problems.push(`${L}: oac sessions reported unbound_attachments ${r.unboundDuring} once the session was bound (expected 0)`);
      if (r.unsteadyAfterBound) problems.push(`${L}: ${r.unsteadyAfterBound} sample(s) taken while it was alive after the session was bound did not list exactly that session with no unbound attachment`);
      const others = r.idsAlive.filter((id) => id !== r.oacSessionId);
      if (others.length) problems.push(`${L}: the samples taken while it was alive listed ${others.length} other session id(s) besides the bound one`);
    }
    if (r.totalAfter !== 0) problems.push(`${L}: ${r.totalAfter} session(s) still listed after it exited`);
    else if (r.unboundAfter !== 0) problems.push(`${L}: oac sessions reported unbound_attachments ${r.unboundAfter} after it exited (expected 0)`);
    if (!r.claudeSessionId) problems.push(`${L}: no Claude Code result with a session id in its own pane text (--output-format json)`);
  }
  const ids = sums.map((r) => r.oacSessionId);
  if (!(ids.every(Boolean) && new Set(ids).size === ids.length)) problems.push('the three runs did not get three distinct OAC session ids (C4 section 6: a resumed or continued session is a new session)');
  if (daemon.accepted !== EXPECTED_CONNECTIONS || daemon.ended !== EXPECTED_CONNECTIONS) problems.push(`the daemon accepted ${daemon.accepted} and ended ${daemon.ended} connections (expected ${EXPECTED_CONNECTIONS} of each: a shim and a hook handler per run)`);
  if (daemon.bindingFindings.length) problems.push(`the daemon logged ${daemon.bindingFindings.length} binding Finding line(s): ${daemon.bindingFindings.join(' | ')}`);
  if (!daemon.stopped) problems.push('the daemon did not report a clean stop');
  return problems;
}

// The daemon's own account, from its events: totals, binding lines, and per run (by the daemon's clock at the
// run's start) the connections and the SC-ID-142 diagnostics. Diagnostics are recorded, never judged.
export function daemonSummary(eventsText, runs) {
  const all = daemonEvents(eventsText);
  const lines = all.filter((e) => e.event === 'log' && typeof e.line === 'string').map((e) => e.line);
  const bindingLines = lines.filter((l) => /binding/.test(l));
  const starts = runs.map((r) => ({ label: r.label, t0: r.t0 }));
  // An event at exactly a run's t0 is the previous run's: t0 is the daemon's clock at its last event before the run.
  const owner = (ms) => starts.filter((s) => s.t0 !== null && s.t0 < ms).at(-1)?.label ?? null;
  const perRun = Object.fromEntries(starts.map((s) => [s.label, { accepted: 0, ended: 0, sc142: [] }]));
  for (const e of all) {
    const label = owner(e.unix_ms);
    if (!label) continue;
    if (e.event === 'accepted') perRun[label].accepted += 1;
    else if (e.event === 'ended') perRun[label].ended += 1;
    else if (e.event === 'log' && typeof e.line === 'string' && /binding\b.*\bSC-ID-142\b/.test(e.line)) perRun[label].sc142.push(e.line);
  }
  return {
    accepted: all.filter((e) => e.event === 'accepted').length,
    ended: all.filter((e) => e.event === 'ended').length,
    bindingLines,
    bindingFindings: lines.filter((l) => /\bbinding Finding\b/.test(l)),
    perRun,
    stopped: all.some((e) => e.event === 'stopped'),
  };
}

export default {
  name: 'g5-claude-resume',
  description: 'Real Claude Code (print mode) started, resumed and continued against the example live daemon: registration and deregistration.',
  harnesses: ['claude'],
  defaults: {
    launch: [],
    timeboxMs: 1200000,
    params: { oac: '', daemon: '', namespace: 'g5r', holdSeconds: '30', waitMs: '60000', claudeWaitMs: '240000' },
  },
  async run(ctx) {
    const { herdr, params } = ctx;
    if (!params.oac || !params.daemon) throw new Error('--param oac=<path> and --param daemon=<path> are required');
    const ns = params.namespace;
    if (!/^[a-z0-9]{1,24}$/.test(ns)) throw new Error('namespace must be 1 to 24 lower-case letters and digits');
    const waitMs = Number(params.waitMs);
    const claudeWaitMs = Number(params.claudeWaitMs);
    const hold = Number(params.holdSeconds);
    if (!Number.isInteger(hold) || hold < 10 || hold > 300) throw new Error('holdSeconds must be an integer from 10 to 300');
    const work = ctx.dir('g5r');
    const events = join(work, 'events.jsonl');
    const mcpPath = join(work, 'mcp.json');
    const settingsPath = join(work, 'settings.json');
    const slash = (p) => p.replace(/\\/g, '/');
    const exitVar = ctx.paneShell === 'powershell' ? '$LASTEXITCODE' : '$?';

    writeFileSync(mcpPath, JSON.stringify({ mcpServers: { oac: { command: params.oac, args: ['mcp-shim', '--harness', 'claude'] } } }));
    writeFileSync(
      settingsPath,
      JSON.stringify({ hooks: { SessionStart: [{ hooks: [{ type: 'command', command: `"${slash(params.oac)}" hook` }] }] } }),
    );
    ctx.record('g5', { nonVerdictBearing: 'G5 #66 box 4: not a gate; records what the daemon and oac sessions reported, and never changes a gate verdict' });

    // Redaction, registered the moment a value is first seen (before any capture that could hold it).
    const claudeIds = new Map();
    const oacIds = new Map();
    const oacPerLabel = new Map();
    function learnClaude(text) {
      for (const id of sessionIdsIn(text)) {
        if (claudeIds.has(id) || oacIds.has(id)) continue;
        const placeholder = `<CLAUDE_SESSION_${claudeIds.size + 1}>`;
        claudeIds.set(id, placeholder);
        ctx.redactLiteral(id, placeholder);
      }
    }
    function learnOac(label, text, o) {
      const ids = [...(Array.isArray(o?.sessions) ? o.sessions.map((s) => s?.session_id) : []), ...sessionIdsIn(text), ...oacIdsIn(text)];
      for (const id of ids) {
        if (typeof id !== 'string' || !id || oacIds.has(id) || claudeIds.has(id)) continue;
        const k = (oacPerLabel.get(label) ?? 0) + 1;
        oacPerLabel.set(label, k);
        const placeholder = k === 1 ? `<OAC_SESSION_${label}>` : `<OAC_SESSION_${label}_${k}>`;
        oacIds.set(id, placeholder);
        ctx.redactLiteral(id, placeholder);
      }
    }
    // A text for the run manifest with every id learned so far replaced (the driver redacts the manifest as well; this is
    // so that nothing here depends on that).
    const scrub = (t) => [...claudeIds, ...oacIds].reduce((acc, [id, ph]) => acc.split(id).join(ph), t);
    // Text that may hold either kind of id (Claude prints the whoami OAC id in prose; the daemon's binding lines print
    // session=...): the Claude ids by their key, and every token of the OAC session id shape (core/src/ids.rs).
    function learnText(label, text) {
      learnClaude(text);
      learnOac(label, text, null);
    }

    // Pane A: the live daemon, in the foreground. `stopped` is true once there is no daemon left to type `stop` into:
    // it stopped, failed, or was stopped by us. The error path never types `stop` into such a pane.
    const wsA = await herdr.workspaceCreate({ cwd: work, label: 'oac-g5r-daemon', env: { OAC_SCOPE: ns } });
    ctx.record('daemonPane', wsA);
    const readDaemon = (lines = 400) => herdr.paneRead(wsA.paneId, { source: 'recent-unwrapped', lines });
    let stopped = false;
    let stopTried = false;
    const stopDaemon = async () => {
      if (stopped || stopTried) return;
      stopTried = true;
      let text = '';
      try {
        text = await readDaemon(200);
      } catch {
        /* the pane may be gone: try the stop anyway */
      }
      if (/"event":"(?:stopped|failed)"/.test(text)) {
        stopped = true;
        return;
      }
      await herdr.paneRun(wsA.paneId, 'stop');
      await herdr.paneWaitOutput(wsA.paneId, { regex: regexLiteral('"event":"stopped"'), source: 'recent-unwrapped', timeoutMs: waitMs });
      stopped = true;
    };
    let daemonCaptured = false;
    let notReadyCaptured = false;
    async function captureDaemon() {
      if (daemonCaptured) return null;
      daemonCaptured = true;
      if (!notReadyCaptured) {
        const pane = await readDaemon(200);
        learnText('daemon', pane);
        ctx.capture('pane-daemon-herdr.txt', pane);
      }
      let text = null;
      try {
        text = readFileSync(events, 'utf8');
      } catch {
        /* the daemon never wrote an events file */
      }
      if (text !== null) {
        learnText('daemon', text);
        ctx.capture('events.jsonl.txt', text);
      }
      return text;
    }

    const runs = [];
    let observedRecorded = false;
    try {
      await herdr.paneRun(wsA.paneId, ctx.quoteForPane([params.daemon, '--namespace', ns, '--events', events]));
      try {
        await herdr.paneWaitOutput(wsA.paneId, { regex: regexLiteral('"event":"ready"'), source: 'recent-unwrapped', timeoutMs: waitMs });
      } catch (err) {
        const pane = await readDaemon(200);
        learnText('daemon', pane);
        ctx.capture('pane-daemon-not-ready-herdr.txt', pane);
        notReadyCaptured = true; // the pane is captured here: no second, duplicate capture of it (the events file still is)
        // A timeout halts the driver's input, so the stop in the error path below is refused then; teardown's process
        // accounting ends the daemon (and every other pane process) on that path.
        throw err;
      }

      // Pane O: `oac sessions --json` in the test namespace.
      const wsO = await herdr.workspaceCreate({ cwd: work, label: 'oac-g5r-ops', env: { OAC_SCOPE: ns } });
      ctx.record('opsPane', wsO);
      let n = 0;
      // One session list. Every id in it is registered for redaction before its capture is written, and the capture is
      // written here, at its own step, so a run that fails later keeps it.
      async function sessions(label, kind, trigger) {
        n += 1;
        const tag = `${label}-${kind}`;
        const id = `${tag}${n}-${randomBytes(3).toString('hex')}`;
        const cmd = ctx.quoteForPane([params.oac, 'sessions', '--json']);
        await herdr.paneRun(wsO.paneId, `echo G5R-${id}-BEGIN; ${cmd}; echo G5R-${id}-EXIT-${exitVar}`);
        await herdr.paneWaitOutput(wsO.paneId, { regex: `^\\s*G5R-${regexLiteral(id)}-EXIT-\\d+\\s*$`, timeoutMs: waitMs });
        const lines = (await herdr.paneRead(wsO.paneId, { source: 'recent-unwrapped', lines: 400 })).split(/\r?\n/);
        const b = lines.findIndex((l) => new RegExp(`^\\s*G5R-${regexLiteral(id)}-BEGIN\\s*$`).test(l));
        const e = lines.findIndex((l) => new RegExp(`^\\s*G5R-${regexLiteral(id)}-EXIT-\\d+\\s*$`).test(l));
        if (b < 0 || e < b) throw new Error(`sessions ${id}: markers not found in the pane text`);
        const text = lines.slice(b + 1, e).join('\n');
        const code = Number(lines[e].trim().split('-').pop());
        const o = jsonLines(text).filter((x) => x && x.oac_cli === 1).at(-1) ?? null;
        learnOac(label, text, o);
        ctx.capture(`sessions-${n}-${tag}.txt`, `$ oac sessions --json\n${text}\nexit ${code}\n`);
        return { n, tag, trigger, code, text, o };
      }

      // Pane B: the real Claude Code, one working directory for all three runs.
      const wsB = await herdr.workspaceCreate({ cwd: ctx.dir('claude-cwd'), label: 'oac-g5r-claude', env: { OAC_SCOPE: ns } });
      ctx.record('claudePane', wsB);
      await ctx.probeEnv(wsB.paneId, { timeoutMs: waitMs });
      // No double quote in the prompt (PowerShell 5.1 drops it; see g10-cli-verbs).
      const prompt =
        `Call the whoami tool of the oac MCP server once. Then run the Bash command sleep ${hold} (exactly that command). ` +
        'Then print the whoami result text exactly, nothing else.';
      if (ctx.paneShell === 'powershell' && prompt.includes('"')) throw new Error('the prompt must not contain a double quote under the PowerShell pane shell');
      const readClaude = () => herdr.paneRead(wsB.paneId, { source: 'recent-unwrapped', lines: 400 });

      async function claudeRun(label, extra) {
        const nonce = randomBytes(4).toString('hex');
        const begin = `G5R-${label}-BEGIN-${nonce}`;
        const mark = `G5R-${label}-DONE-${nonce}`;
        // The daemon's clock at its last event so far: this run's events are the ones after it.
        const t0 = daemonClockMs(await readDaemon(400)) ?? Date.now();
        const run = { label, t0, samples: [], after: null, claudeSessionId: null, sleepFailed: false };
        runs.push(run);
        const claude = ctx.quoteForPane([
          'claude', '-p', prompt, ...extra,
          '--output-format', 'json',
          '--allowedTools', 'mcp__oac__whoami', 'Bash(sleep *)',
          '--mcp-config', mcpPath, '--strict-mcp-config', '--settings', settingsPath,
          '--max-turns', '6',
        ]);
        // The begin marker comes first so that this run's text can be cut out of the shared pane.
        await herdr.paneRun(wsB.paneId, `echo ${begin}; ${claude}; echo ${mark}`);

        // Own text of the run so far (never an earlier run's).
        const ownText = async () => sliceFromToken(await readClaude(), begin);
        try {
          // While it is alive, on the daemon's events: its first connection accepted, then the hook handler's freed.
          const take = async (trigger) => {
            const s = await sessions(label, 'during', trigger);
            // Alive only if the run's done marker has not appeared by the end of the sample.
            s.alive = !hasLine((await ownText()) ?? (await readClaude()), mark);
            run.samples.push(s);
            return s;
          };
          for (const [trigger, regex] of [['accepted', eventAfterRegex('accepted', t0)], ['hook-freed', freedAfterRegex(t0)]]) {
            await herdr.paneWaitOutput(wsA.paneId, { regex, source: 'recent-unwrapped', timeoutMs: claudeWaitMs });
            await take(trigger);
          }
          // The hook's freed line comes after its disconnect, not at the bind (see the header): when nothing is listed
          // yet and the run is still alive, re-list a bounded number of times, each started by the previous sample.
          for (let k = 1; k <= MAX_BIND_RETRIES && !run.samples.some(isBoundSample) && run.samples.at(-1).alive; k += 1) await take(`bind-retry-${k}`);
          await herdr.paneWaitOutput(wsB.paneId, { regex: `^\\s*${regexLiteral(mark)}\\s*$`, timeoutMs: claudeWaitMs });
        } catch (err) {
          try {
            // Without the begin marker (scrolled away, say) the whole pane is the evidence: its ids are learned first.
            const text = (await ownText()) ?? (await readClaude());
            learnText(label, text);
            ctx.capture(`pane-claude-${label}-not-done-herdr.txt`, text);
          } catch {
            /* the evidence is best effort on this path; the original error is the one to report */
          }
          throw err;
        }
        const claudeText = await ownText();
        if (claudeText === null) throw new Error(`run ${label}: its begin marker was not found in the Claude pane text; its own text cannot be cut out`);
        learnText(label, claudeText);
        ctx.capture(`pane-claude-${label}-herdr.txt`, claudeText);
        run.claudeSessionId = claudeResults(claudeText).at(-1)?.session_id ?? null;
        run.sleepFailed = /missing operand/i.test(claudeText);

        // After it exited: the shim's connection freed (the daemon's own "freed" line, after `disconnect` returned).
        const hookFreed = firstFreedAfter(await readDaemon(400), t0);
        await herdr.paneWaitOutput(wsA.paneId, { regex: freedAfterRegex(hookFreed ?? t0), source: 'recent-unwrapped', timeoutMs: waitMs });
        run.after = await sessions(label, 'after', 'shim-freed');
        return run;
      }

      const a = await claudeRun('A', []);
      if (!a.claudeSessionId) throw new Error('run A: no Claude session id in its --output-format json result; cannot resume');
      await claudeRun('B', ['--resume', a.claudeSessionId]);
      await claudeRun('C', ['--continue']);

      const sums = runs.map(summarizeRun);
      ctx.record('observed', {
        runs: sums.map((r) => observedRun(r, a.claudeSessionId)),
        oacSessionIdsDistinct: sums.every((r) => r.oacSessionId) && new Set(sums.map((r) => r.oacSessionId)).size === sums.length,
      });
      observedRecorded = true;

      await stopDaemon();
      const eventsText = (await captureDaemon()) ?? '';
      const daemon = daemonSummary(eventsText, runs);
      ctx.record('daemon', {
        accepted: daemon.accepted,
        ended: daemon.ended,
        bindingLines: daemon.bindingLines.map(scrub),
        bindingFindings: daemon.bindingFindings.map(scrub),
        perRun: Object.fromEntries(Object.entries(daemon.perRun).map(([k, v]) => [k, { ...v, sc142: v.sc142.map(scrub) }])),
        stopped: daemon.stopped,
      });

      const problems = judgeG5(runs, daemon);
      problems.forEach((p) => ctx.finding(scrub(p)));
      if (problems.length) throw new Error('G5 resume leg expectations not met; see the findings and the captures');
    } catch (err) {
      if (!observedRecorded && runs.length) ctx.record('observed', { incomplete: true, runs: runs.map((r) => observedRun(summarizeRun(r), runs[0]?.claudeSessionId)) });
      try {
        await stopDaemon();
      } catch {
        /* already stopped, input halted after a timeout, or the pane is gone */
      }
      try {
        await captureDaemon();
      } catch {
        /* best effort */
      }
      throw err;
    }
  },
};
