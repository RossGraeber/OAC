// s3-codex-capture: the Stage 1 fixture capture for Gate S3 criterion 5 (#343), driven
// through herdr. NOT A GATE RUN and NOT VERDICT-BEARING: it records live Codex app-server
// behaviour that the fake Codex app-server (tests/fakes/codex-app-server/) had modelled from
// source only, so each fake behaviour traces to a recorded fixture. It never changes a gate
// verdict, STATUS.md's gate table, or PINS.md.
//
// It reuses the G2 scenario's launch, dialog, readiness and thread-identification steps
// (scenarios/g2-codex-inject.mjs, lib/g2.mjs) unchanged, and runs its own quarantined client
// (docs/planning/gates/fixtures/s3-codex-capture/client.mjs.throwaway-quarantined) instead of
// the G2 one. That client keeps the G2 client's transport, transcript format and `list`,
// `watch` and `turns` modes, and adds `idleadd` and `cases`.
//
// Operator command (herdr at the PINS.md pin, a signed-in Codex CLI of any version: versions
// float, a difference from PINS.md is a VERSION WARNING finding, never a stop, #216):
//
//   node tools/herdr/run.mjs --scenario s3-codex-capture --out <run dir>
//
// What it does:
//   0-5. As g2-codex-inject: preflight (plain `codex` launch, versions against PINS.md as
//        committed, the client staged from its blob at HEAD and its sha256 checked),
//        `codex app-server daemon start` (never stopped), the pre-launch `list`, the plain
//        launch through herdr with Codex's recorded trust dialog the only one the driver
//        accepts, the operator's own message, the TUI's thread found on the wire, `watch`
//        subscribed to it, and the thread idle on the event stream.
//   6a. The idle add: the client's `idleadd` sends ONE thread/queue/add to the TUI's idle,
//       loaded thread, whose last turn completed (it is never interrupted). Nothing else is
//       sent to that thread first. The watch stream must then show the queued message start a
//       turn (its userMessage carries clientId = clientUserMessageId) and that turn complete.
//   6b. The cases: the client's `cases` runs, in a scratch directory of its own, on threads it
//       starts itself: a request before initialize; thread/queue/add without
//       experimentalApi; thread/resume and thread/queue/add of thread ids no thread has; an add
//       to an ephemeral thread; two adds during a running turn; an add with an extra member;
//       turn/interrupt and an add to the thread it left interrupted; an add to a thread the
//       daemon unloaded, then loaded again; an add to an archived thread; and (#343 review)
//       two adds during a running turn that is then interrupted. Every wait in the
//       client is bounded; its only fixed waits are observation windows for "nothing
//       happens" (--param quietMs, default 25000). An error answer is an observation here,
//       never a divergence. --param cases=queued-interrupt runs only the last case, skips 6a,
//       and names its captures `*-<version>-queued-interrupt-herdr.*`.
//   6c. The client's `turns` on the TUI's thread.
//   7.  Post-run versions, as G2.
//   Captures: `transcript-<date>-<version>-herdr.jsonl` (the client's own wire transcript,
//   sanitized as G2's) and `pane-<date>-<version>-herdr.txt`.
//
// Credentials: as g2-codex-inject, nothing here or in the client reads a file under the Codex
// home or asks an OS credential store for anything.

import { existsSync, readFileSync, realpathSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { NotRunError, DriverError } from '../lib/herdr.mjs';
import { parseCodexVersions, pinsReadWarning, parseCodexCliVersion, parseCodexDaemonVersion, codexVersionWarning, CODEX_PIN_ROW, CODEX_DAEMON_VERSION_FIELDS } from '../lib/pins.mjs';
import { harnessVersions } from '../lib/manifest.mjs';
import { runBounded, spawnLongRunning, killTree, descendants, within } from '../lib/proc.mjs';
import { CODEX_DAEMON_SCRATCH_HOLDER } from '../lib/scratch.mjs';
import { committedFile, formatSection, sameDialog, acceptHint } from '../lib/g1.mjs';
import { driverAcceptDialog, recordWaitState, settleAfterObservation, pastFloor, refuseRunningTurn } from '../lib/gate-common.mjs';
import {
  G2_LAUNCH, PINS_PATH, DEFAULT_OPERATOR_PROMPT, assertNotInjected, stageClientCopy,
  fixtureNames, unverifiedNames, classifyCodexScreen, driverMayAcceptCodex, normalizeDialogText, paneArgv, parseG2Transcript,
  g2Facts, identifyTuiThread, sanitizeTranscript, CODEX_DIALOG_KINDS, waitCodexReady, loadedSince, codexReadyTimeoutFinding, multipleNewThreadsFinding,
} from '../lib/g2.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..', '..');
const AGENT = 's3codex';
// The quarantined capture client, and the sha256 of the blob this scenario was written for.
export const COMMITTED_CLIENT = 'docs/planning/gates/fixtures/s3-codex-capture/client.mjs.throwaway-quarantined';
export const COMMITTED_CLIENT_SHA256 = 'b110a9d3f4b4c328f964dead0c6e6cd5374be2eb58cc5e6a06a6c6e33fff07e5';
export const IDLE_ADD_TEXT = 'OAC S3 capture idle add: reply with exactly the words OAC S3 IDLE ADD RECEIVED. Do not use any tools.';
// The label this scenario's version findings carry (lib/pins.mjs defaults it to G2), and what
// the run manifest says about the run's standing. Added after run 20261007T221317Z-1a3890,
// whose manifest carries G2's text (a copy-paste defect; S3-codex-2026-10-07.md says so).
export const S3_GATE = 'S3 capture';
export const S3_NON_VERDICT = '#343: a Stage 1 fixture capture for Gate S3 criterion 5; compared against no other run, not a gate run, never changes a gate verdict';
// The client's case sets: `all`, or only `queued-interrupt` (items already queued when a turn
// is interrupted; #343 review, finding 1). A set other than `all` skips the idle add.
export const CASE_SETS = Object.freeze(['all', 'queued-interrupt']);

// Each delivery happens once, whatever happens next: nothing is ever re-sent. `log` gets one
// entry per send.
export function onceGuard(log) {
  const sent = new Set();
  return (what) => {
    if (sent.has(what)) throw new DriverError(`refusing to send ${what} a second time; nothing is re-sent`);
    sent.add(what);
    log.push({ what, at: new Date().toISOString() });
  };
}

// The idle add's precondition, from the daemon's own turn record (a `turnsLists` entry of
// lib/g2.mjs g2Facts): the thread's last turn completed. -> null, or why the add must not run.
export function idleAddRefusal(priorTurns) {
  const last = priorTurns?.turns?.[0] ?? null;
  if (last && last.status === 'completed') return null;
  return `the TUI thread's last turn is ${last ? `\`${last.status}\`` : 'not on record'} (thread/turns/list line ${priorTurns?.line ?? '?'}), not \`completed\`; the idle-add case needs a thread whose last turn completed; nothing sent`;
}

// The Codex versions the wire reported, one per connection that sent initialize. The
// `cases-preinit` connection never does (it is the "Not initialized" case), so it reports
// none and is left out (c1de8f4: counting it kept every capture `unverified-*`).
export function wireVersionsOf(connections) {
  return [...new Set(connections.filter((c) => c.mode !== 'cases-preinit').map((c) => c.userAgentVersion))];
}

// Capture names: G2's for the full case set, with the set's name before `-herdr` otherwise,
// so a run of one set never takes the name of a full run of the same day and version.
const withSet = (names, set) => (set === 'all' ? names : Object.fromEntries(Object.entries(names).map(([k, v]) => [k, v.replace(/-herdr\.(\w+)$/, `-${set}-herdr.$1`)])));
export const s3FixtureNames = (date, version, set = 'all') => withSet(fixtureNames(date, version), set);
export const s3UnverifiedNames = (date, set = 'all') => withSet(unverifiedNames(date), set);
const INPUT_ROLES = new Set(['operator-input', 'dialog-accept']);
const sleep = (ms) => new Promise((r) => setTimeout(r, Math.max(0, ms)));
const cap = (s, n) => (String(s ?? '').length > n ? `${String(s).slice(0, n)}… (${String(s).length} chars)` : String(s ?? ''));

export default {
  name: 's3-codex-capture',
  description: 'Stage 1 fixture capture for Gate S3 criterion 5 (#343): live Codex app-server behaviour the fake modelled from source. Not a gate run; not verdict-bearing.',
  harnesses: ['codex'],
  defaults: {
    launch: [...G2_LAUNCH],
    timeboxMs: 60 * 60 * 1000, // declared before the run: the G2 box plus the cases' 15 minutes
    params: {
      accept: 'driver', // #196: the driver accepts recognized dialogs by default; accept=human remains
      operatorPrompt: DEFAULT_OPERATOR_PROMPT,
      idleAddText: IDLE_ADD_TEXT,
      cases: 'all', // CASE_SETS
      quietMs: '25000', // the client's "nothing happens" window
      casesTimeoutMs: '1500000',
      startupTimeoutMs: '120000',
      humanAcceptTimeoutMs: '300000',
      cliTimeoutMs: '60000',
      clientTimeoutMs: '60000',
      wireTimeoutMs: '30000',
      readyTimeoutMs: '120000', // #204: bound on the verified-ready wait before the operator's message
      attachTimeoutMs: '180000',
      listPollMs: '3000',
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
    const busyIndicator = params.busyIndicator;
    const pollMs = num('pollMs');
    const caseSet = params.cases || 'all';
    if (!CASE_SETS.includes(caseSet)) throw new DriverError(`--param cases must be one of ${CASE_SETS.join(', ')}`);
    const operatorPrompt = params.operatorPrompt || DEFAULT_OPERATOR_PROMPT;
    try {
      assertNotInjected('operatorPrompt', operatorPrompt);
    } catch (err) {
      throw new DriverError(err.message);
    }

    const s3 = {
      nonVerdictBearing: S3_NON_VERDICT,
      acceptPolicy: accept,
      params: { ...params },
      launch: { expected: [...G2_LAUNCH], actual: launch, verbatim: null },
      versions: null,
      client: null,
      date: new Date().toISOString().slice(0, 10),
      fixtures: null, // K7 fixture names: set only after the CLI, daemon and wire versions are verified
      captureNames: null,
      daemon: { start: null, versionBefore: null, versionAfter: null },
      clientRuns: [],
      preLaunch: null,
      agentStart: null,
      paneArgv: null,
      dialogs: [],
      herdrStates: [],
      operatorInput: null,
      thread: null,
      watch: null,
      idle: null,
      idleAdd: null,
      cases: null,
      turnsList: null,
      injectionsSent: [],
      divergence: [],
      postRun: null,
      sanitizer: null,
      stoppedAt: null,
    };
    const sections = [];
    let clientDir = null;
    let projectDirs = [];
    let lastKeptKey = null;
    let watchProc = null;
    const abortSignal = herdr.abortSignal;

    const stop = (reason) => {
      herdr.inputHalted ??= reason;
      s3.stoppedAt = reason;
      throw new NotRunError(reason);
    };
    const diverge = (what) => {
      const msg = `the committed S3 capture client did not work unmodified on this Codex version: ${what}. Recorded as a divergence; the client is not patched to get past it`;
      s3.divergence.push(what);
      ctx.finding(msg);
      throw new DriverError(msg);
    };
    const aborted = () => {
      if (abortSignal?.aborted) throw new NotRunError(herdr.abortReason());
    };

    // --- pane reads (verbatim, kept with their herdr command) ---------------------------
    const keptSeqs = new Set();
    const keepSection = (sec, text) => {
      if (keptSeqs.has(sec.seq)) return;
      sections.push(formatSection(sec, text));
      keptSeqs.add(sec.seq);
      lastKeptKey = `${sec.source}\n${text}`;
    };
    const read = async (label, { source = 'visible', lines, keep = true } = {}) => {
      const text = await herdr.agentRead(AGENT, { source, lines, deadlineMs: 15000 });
      const e = herdr.commands.at(-1);
      const sec = { seq: e.seq, label, source, startedAt: e.startedAt, endedAt: e.endedAt };
      const key = `${source}\n${text}`;
      if (keep === true || key !== lastKeptKey) keepSection(sec, text);
      return { ...sec, text, screen: classifyCodexScreen(text, { busyIndicator }) };
    };
    const deadlineFor = (ms) => Date.now() + Math.min(ms, Math.max(0, ctx.remainingMs()));
    // Time spent waiting for the operator to accept a dialog: bounded by humanAcceptTimeoutMs
    // on its own, so an enclosing settle/wire wait does not also charge it to its budget (#154).
    let humanWaitMs = 0;
    const leftUntil = (deadline, waitedAtStart) => Math.min(deadline + humanWaitMs - waitedAtStart, Date.now() + Math.max(0, ctx.remainingMs())) - Date.now();

    // --- driver child processes: the Codex CLI and the committed client -----------------
    const codexCli = async (args, label) => {
      aborted();
      const [file, argv] = process.platform === 'win32' ? ['cmd.exe', ['/d', '/s', '/c', 'codex', ...args]] : ['codex', args];
      const r = await runBounded(file, argv, { deadlineMs: Math.min(num('cliTimeoutMs'), Math.max(1, ctx.remainingMs())), env: process.env, abortSignal });
      const rec = { label, argv: ['codex', ...args], startedAt: r.startedAt, endedAt: r.endedAt, exitCode: r.exitCode, timedOut: r.timedOut, spawnError: r.spawnError };
      aborted();
      if (r.timedOut) stop(`\`codex ${args.join(' ')}\` timed out after ${num('cliTimeoutMs')} ms; nothing re-run`);
      return { r, rec };
    };

    const transcriptPath = () => join(clientDir, 'transcript.jsonl');
    const transcriptEntries = () => (clientDir && existsSync(transcriptPath()) ? parseG2Transcript(readFileSync(transcriptPath(), 'utf8'), { completeLinesOnly: true }) : []);
    const lineCount = () => transcriptEntries().at(-1)?.line ?? 0;
    const facts = () => g2Facts(transcriptEntries());

    // What one client process run left on the wire, and whether it worked unmodified.
    // `cases` opens several connections, each named `cases-<label>`; their error answers are
    // the observations the run is for, never problems. `cases-preinit` sends no initialize.
    const clientProblems = (rec) => {
      const f = facts();
      const ofRun = (c) => (rec.mode === 'cases' ? c.mode.startsWith('cases-') : c.mode === rec.mode);
      const mine = f.connections.filter((c) => ofRun(c) && c.firstLine > rec.linesBefore);
      const out = [];
      if (!mine.length) out.push('no connection on the wire (no WebSocket upgrade recorded)');
      for (const c of mine) {
        if (c.upgraded === false) out.push(`WebSocket upgrade refused: ${c.handshake}`);
        if (!c.userAgent && c.mode !== 'cases-preinit') out.push('initialize returned no userAgent');
        if (c.unparsed) out.push(`${c.unparsed} unparsed frame(s)`);
        if (rec.mode !== 'cases') for (const e of c.errors) out.push(`${e.method} answered with error ${JSON.stringify(e.error)}`);
      }
      if (rec.mode !== 'watch' && rec.exitCode !== 0) out.push(`client exited ${rec.exitCode ?? rec.signal ?? 'abnormally'}`);
      return { problems: out, connections: mine };
    };

    const clientRecord = (mode, args, r, linesBefore) => ({
      mode,
      args,
      startedAt: r?.startedAt ?? null,
      endedAt: r?.endedAt ?? null,
      exitCode: r?.exitCode ?? null,
      signal: r?.signal ?? null,
      timedOut: r?.timedOut ?? null,
      // Not the client's stdout: `list` prints other saved sessions' previews. Only its
      // own status lines are kept; the wire transcript holds everything else.
      stdoutBytes: r ? Buffer.byteLength(r.stdout ?? '') : null,
      stdoutStatus: r ? String(r.stdout ?? '').split('\n').filter((l) => /^\[(?:done|error|upgrade failed|case|server-request ignored[^\]]*)\]/.test(l)).map((l) => cap(l.trimEnd(), 600)) : [],
      stderr: r ? cap(String(r.stderr ?? '').trim(), 2000) : null,
      linesBefore,
      linesAfter: null,
      problems: [],
    });

    const runClient = async (mode, args, { deadlineMs = num('clientTimeoutMs') } = {}) => {
      aborted();
      const linesBefore = lineCount();
      const r = await runBounded(process.execPath, [join(clientDir, 'client.mjs'), mode, ...args], { deadlineMs: Math.min(deadlineMs, Math.max(1, ctx.remainingMs())), env: process.env, cwd: clientDir, abortSignal });
      const rec = clientRecord(mode, args, r, linesBefore);
      rec.linesAfter = lineCount();
      s3.clientRuns.push(rec);
      aborted();
      if (r.timedOut) stop(`the client's \`${mode}\` run timed out after ${deadlineMs} ms; nothing re-sent`);
      if (r.spawnError) throw new DriverError(`could not run the staged client (${r.spawnError})`);
      rec.problems = clientProblems(rec).problems;
      if (rec.problems.length) diverge(`\`${mode}\`: ${rec.problems.join('; ')}`);
      return rec;
    };

    // Each delivery happens once, whatever happens next: nothing is ever re-sent.
    const once = onceGuard(s3.injectionsSent);

    // --- dialogs: text on record before any keystroke ------------------------------------
    const handleDialog = async (r, context) => {
      keepSection(r, r.text);
      if (s3.dialogs.length >= num('maxDialogs')) stop(`more than ${num('maxDialogs')} dialogs; stopping`);
      const kind = r.screen.dialog;
      const d = { index: s3.dialogs.length + 1, kind, context, patternVerified: CODEX_DIALOG_KINDS[kind]?.verified ?? null, readSeq: r.seq, readAt: r.startedAt, selected: r.screen.selected, acceptOrigin: null, acceptSeq: null, resolvedSeq: null, inputBetweenReadAndAccept: null };
      s3.dialogs.push(d);
      if (accept === 'driver') {
        await driverAcceptDialog({ herdr, target: AGENT, r, d, kind, dialogKinds: CODEX_DIALOG_KINDS, plan: driverMayAcceptCodex(r.screen), read, stop, num, sleep, deadlineFor });
        return;
      }
      const before = herdr.commands.length;
      console.error(
        `\n[s3-codex-capture] dialog ${d.index} (${kind}) is on screen; its text is recorded (herdr command #${r.seq}).\n` +
          acceptHint({ sessionName: ctx.sessionName, agent: AGENT, kind, selected: r.screen.selected, dialogKinds: CODEX_DIALOG_KINDS }) +
          `  The driver sends no keystroke to it and waits up to ${num('humanAcceptTimeoutMs')} ms.\n`,
      );
      const waitStart = Date.now();
      const deadline = deadlineFor(num('humanAcceptTimeoutMs'));
      d.redrawSeqs = [];
      let shown = normalizeDialogText(r.text);
      try {
        for (;;) {
          if (Date.now() >= deadline) stop(`dialog ${d.index} (${kind}) was not accepted by the operator within ${num('humanAcceptTimeoutMs')} ms`);
          await sleep(pollMs);
          const p = await read(`dialog-${d.index}-waiting`, { keep: 'on-change' });
          // A redraw of the same dialog is not an answer to it (#160): recorded, wait goes on.
          if (sameDialog(r.text, p, kind, CODEX_DIALOG_KINDS)) {
            const now = normalizeDialogText(p.text);
            if (now !== shown) d.redrawSeqs.push(p.seq);
            shown = now;
            continue;
          }
          d.acceptOrigin = 'human';
          d.resolvedSeq = p.seq;
          d.inputBetweenReadAndAccept = herdr.commands.slice(before).filter((c) => INPUT_ROLES.has(c.role)).length;
          return;
        }
      } finally {
        d.humanWaitMs = Date.now() - waitStart;
        humanWaitMs += d.humanWaitMs;
      }
    };

    // herdr agent state: recorded, and used only to decide when to read next. `unknown` is
    // requested explicitly (K1 §5 item 5) so a wait cannot hang on it, and it never leads
    // to anything being sent again.
    // #253: a wait whose answer carries no agent_status ends the run NOT RUN (recordWaitState).
    const waitState = async (context, timeoutMs) => {
      const w = await herdr.agentWait(AGENT, { until: ['idle', 'done', 'blocked', 'unknown'], timeoutMs: Math.max(1000, timeoutMs) });
      return recordWaitState({ g: s3, context, w, ctx, stop });
    };

    // Wait until the pane shows neither a dialog nor work in progress. `done` (#253) is the
    // wire-level "turn finished" signal when the scenario has one: with it, a herdr `unknown`
    // counts as settled (recorded with a finding); without it, `unknown` is never settled.
    // #282: `floor` (a state_change_seq) is a settle that must be at or past it.
    const settle = async (context, timeoutMs, { done = null, floor = null } = {}) => {
      if (floor === null) await sleep(num('settleMs'));
      const deadline = deadlineFor(timeoutMs);
      const waited0 = humanWaitMs;
      let blockedUnseen = 0;
      for (;;) {
        const left = leftUntil(deadline, waited0);
        if (left <= 0) stop(`${context}: the pane did not settle within ${timeoutMs} ms`);
        const st = await waitState(context, left);
        const state = st.state;
        const r = await read(`${context}-settled?`, { keep: 'on-change' });
        if ((state === 'unknown' && !(done && done())) || !pastFloor(st, { floor })) {
          await sleep(pollMs);
          continue;
        }
        if (!r.screen.dialog && state === 'blocked') {
          if (++blockedUnseen < 3) {
            await sleep(pollMs);
            continue;
          }
          r.screen = { ...r.screen, dialog: 'unknown', selected: null };
        } else blockedUnseen = 0;
        if (r.screen.dialog) {
          await handleDialog(r, context);
          continue;
        }
        if (r.screen.busy) {
          await sleep(pollMs);
          continue;
        }
        return { read: r, state, stateChangeSeq: st.stateChangeSeq, waitSeq: st.seq };
      }
    };

    // #246: a full read after a turn the wire showed completed: settle first, retry the read
    // after another settle if herdr refuses it (agent_not_idle), never re-send anything. If
    // herdr reports `unknown` throughout, fall back to the visible screen (herdr's documented
    // alternative), with a finding.
    // doneEvidence: the watch-stream event that showed the turn over (named in any finding).
    const settledRead = async (label, { source = 'recent-unwrapped', lines }, context, doneEvidence) => {
      for (let refusals = 0; ; ) {
        const s = await settle(context, num('turnTimeoutMs'), { done: () => doneEvidence });
        const res = await herdr.agentReadResult(AGENT, { source, lines, deadlineMs: 15000, allowErrorCodes: ['agent_not_idle'] });
        const e = res.entry;
        if (res.errorCode !== 'agent_not_idle') {
          const sec = { seq: e.seq, label, source, startedAt: e.startedAt, endedAt: e.endedAt };
          keepSection(sec, res.text);
          return { ...sec, text: res.text, screen: classifyCodexScreen(res.text, { busyIndicator }) };
        }
        s3.notIdleRefusals = [...(s3.notIdleRefusals ?? []), { context, seq: e.seq, settledState: s.state }];
        if (s.state === 'unknown') {
          ctx.finding(`herdr refused the ${source} read (${context}, herdr command #${e.seq}, agent_not_idle) while it reported \`unknown\`; the turn was over by ${doneEvidence}. The read fell back to the visible screen, so this capture holds less history`);
          return { ...(await read(label)), fellBackToVisible: true };
        }
        if (++refusals >= 3) stop(`${context}: herdr refused the ${source} read ${refusals} times (agent_not_idle, last #${e.seq}) after the pane settled; nothing more sent`);
      }
    };

    // Wait for something on the client's wire transcript, reading the pane every pollMs
    // meanwhile (kept when it changed) and handling any dialog.
    const waitWire = async (what, pred, timeoutMs, { label = what, bail = null } = {}) => {
      const deadline = deadlineFor(timeoutMs);
      const waited0 = humanWaitMs;
      let nextRead = 0;
      for (;;) {
        aborted();
        const hit = pred(facts());
        if (hit) return hit;
        const b = bail?.();
        if (b) return { bailed: b };
        if (leftUntil(deadline, waited0) <= 0) stop(`timed out after ${timeoutMs} ms waiting for ${what} on the app-server event stream`);
        if (Date.now() >= nextRead) {
          const r = await read(label, { keep: 'on-change' });
          if (r.screen.dialog) await handleDialog(r, what);
          nextRead = Date.now() + pollMs;
        }
        await sleep(200);
      }
    };

    const recordPaneArgv = async (paneId, context) => {
      // One process table for the pane's whole tree, the one the driver took for its query
      // (#136 review, #244 note D: each table is one WMI query, ~1.6 s, on Windows).
      const { info, table: procTable } = await herdr.paneProcessSnapshot(paneId);
      const fg = (info.foreground_processes ?? []).map((p) => p.pid).filter(Number.isInteger);
      const tree = [...new Set([...fg, ...fg.flatMap((p) => descendants(p, procTable) ?? []), ...(descendants(info.shell_pid, procTable) ?? [])])];
      // #232: argv minimized (executable basenames, the `codex` token; plain `codex` asserts no
      // argument, so every other argument is a length placeholder); herdr's own answer is kept as
      // pids only, so no command text it may report reaches the record either.
      const { argv, proof } = paneArgv(tree, procTable);
      const herdrProcessInfo = { pane_id: info.pane_id ?? null, shell_pid: info.shell_pid ?? null, foreground_processes: fg.map((pid) => ({ pid })) };
      return { context, seq: herdr.commands.at(-1).seq, herdrProcessInfo, argv, proof };
    };

    try {
      // --- 0. preflight -------------------------------------------------------------------
      s3.launch.verbatim = JSON.stringify(launch) === JSON.stringify(G2_LAUNCH);
      if (!s3.launch.verbatim) {
        throw new DriverError(`launch ${JSON.stringify(launch)} is not plain \`codex\` (${JSON.stringify(G2_LAUNCH)}); G2 needs a TUI launched with no config overrides, so this would not be a G2 re-run`);
      }
      const cliRaw = ctx.harnessVersion('codex');
      const pinsFile = committedFile(REPO, PINS_PATH);
      // Versions float and are never gated (#216): every difference from PINS.md's minimum
      // or last tested version is a VERSION WARNING finding, and the run continues.
      const pin = parseCodexVersions(pinsFile.bytes.toString('utf8'));
      const cli = parseCodexCliVersion(cliRaw);
      s3.versions = {
        pinsRow: CODEX_PIN_ROW,
        pinsMinimum: pin.minimum,
        pinsLastTested: pin.lastTested,
        pinsCommit: pin.commit,
        pinsSource: { path: PINS_PATH, headCommit: pinsFile.headCommit, committedSha256: pinsFile.committedSha256, workingTreeMatchesHead: pinsFile.workingTreeMatchesHead },
        cliOutput: cliRaw,
        cli,
        daemon: null,
        wireUserAgent: null,
        wire: null,
        verified: false, // true once the CLI, the daemon and the wire report one and the same version
        matchesLastTested: null,
        warnings: [],
      };
      const warn = (w) => {
        if (!w) return;
        s3.versions.warnings.push(w);
        ctx.finding(w);
      };
      if (!pinsFile.workingTreeMatchesHead) ctx.finding(`${PINS_PATH} has uncommitted changes; the version check read the committed PINS.md (HEAD ${pinsFile.headCommit})`);
      warn(pinsReadWarning(pin, S3_GATE));
      if (!cli && /^N\/A/.test(cliRaw ?? 'N/A')) stop(`codex --version could not be run (${cliRaw ?? 'not recorded'}); nothing launched`);
      warn(codexVersionWarning({ observed: cli, lastTested: pin.lastTested, minimum: pin.minimum, source: '`codex --version`', gate: S3_GATE }));
      s3.captureNames = s3UnverifiedNames(s3.date, caseSet);

      clientDir = ctx.dir('s3-client');
      const staged = stageClientCopy(REPO, COMMITTED_CLIENT, clientDir);
      s3.client = {
        committed: COMMITTED_CLIENT,
        headCommit: staged.headCommit,
        gitMode: staged.mode,
        committedSha256: staged.committedSha256,
        expectedSha256: COMMITTED_CLIENT_SHA256,
        workingTreeSha256: staged.workingTreeSha256,
        workingTreeIsSymlink: staged.workingTreeIsSymlink,
        workingTreeMatchesHead: staged.workingTreeMatchesHead,
        copy: staged.copyPath,
        copySha256: staged.copySha256,
        match: staged.match,
      };
      if (staged.committedSha256 !== COMMITTED_CLIENT_SHA256) throw new DriverError(`${COMMITTED_CLIENT} at HEAD has sha256 ${staged.committedSha256}, not the ${COMMITTED_CLIENT_SHA256} this scenario was written for; refusing to run`);
      if (!staged.workingTreeMatchesHead) throw new DriverError(`${COMMITTED_CLIENT} in the working tree differs from HEAD (edited, replaced or symlinked); refusing to run`);
      if (!staged.match) throw new DriverError('the staged client copy does not match the committed blob (sha256)');

      const projectDir = ctx.dir('s3-project');
      projectDirs = [...new Set([projectDir, realpathSync(projectDir)])];

      // --- 1. daemon ------------------------------------------------------------------------
      ctx.noteScratchHolder?.(CODEX_DAEMON_SCRATCH_HOLDER);
      const ds = await codexCli(['app-server', 'daemon', 'start'], 'daemon start');
      s3.daemon.start = { ...ds.rec, stdout: cap(ds.r.stdout.trim(), 2000), stderr: cap(ds.r.stderr.trim(), 2000) };
      if (ds.r.spawnError || ds.r.exitCode !== 0) throw new DriverError(`\`codex app-server daemon start\` failed (${ds.r.spawnError ?? `exit ${ds.r.exitCode}`}); nothing launched`);
      const dv = await codexCli(['app-server', 'daemon', 'version'], 'daemon version');
      const daemonV = parseCodexDaemonVersion(dv.r.stdout);
      s3.daemon.versionBefore = { ...dv.rec, parsed: daemonV };
      s3.versions.daemon = daemonV;
      for (const k of CODEX_DAEMON_VERSION_FIELDS) {
        warn(codexVersionWarning({ observed: daemonV?.[k] ?? null, lastTested: pin.lastTested, minimum: pin.minimum, source: `\`codex app-server daemon version\` ${k}`, gate: S3_GATE }));
      }

      // --- 2. pre-launch list; the wire version --------------------------------------------
      const pre = await runClient('list', []);
      const preConn = clientProblems(pre).connections[0];
      const preFacts = facts();
      s3.preLaunch = { run: s3.clientRuns.length - 1, loaded: loadedSince(preFacts.loadedLists, pre.linesBefore) };
      // #205 review: the ready wait needs this baseline; without it nothing is launched.
      if (s3.preLaunch.loaded === null) stop('the pre-launch `thread/loaded/list` could not be read, so the ready wait (#204) could not tell a thread new since the launch; nothing launched');
      s3.versions.wireUserAgent = preConn.userAgent;
      s3.versions.wire = preConn.userAgentVersion;
      warn(codexVersionWarning({ observed: preConn.userAgentVersion, lastTested: pin.lastTested, minimum: pin.minimum, source: 'the wire initialize userAgent', gate: S3_GATE }));
      const sameVersion = !!cli && preConn.userAgentVersion === cli && CODEX_DAEMON_VERSION_FIELDS.every((k) => daemonV?.[k] === cli);
      if (sameVersion) {
        s3.versions.verified = true; // CLI, daemon and wire report one and the same version
        s3.versions.matchesLastTested = cli === pin.lastTested;
        s3.fixtures = s3FixtureNames(s3.date, cli, caseSet);
        s3.captureNames = s3.fixtures;
      } else {
        ctx.finding(`the Codex CLI (${cliRaw}), daemon (${JSON.stringify(daemonV)}) and wire (${preConn.userAgentVersion ?? 'none'}) do not report one and the same version; the run continues, but its captures stay unverified-* because they cannot name one Codex version`);
      }
      const idleAddText = params.idleAddText || IDLE_ADD_TEXT;
      try {
        assertNotInjected('idleAddText', idleAddText);
      } catch (err) {
        throw new DriverError(err.message);
      }

      // --- 3. launch plain codex; the pane's argv; dialogs -----------------------------------
      const ws = await herdr.workspaceCreate({ cwd: projectDir, label: 'oac-s3' });
      ctx.record('workspace', ws);
      await herdr.paneProcessInfo(ws.paneId);
      await ctx.probeEnv(ws.paneId);
      const started = await ctx.startAgent(AGENT, { paneId: ws.paneId, timeoutMs: num('startupTimeoutMs'), allowErrorCodes: ['agent_not_ready'] });
      s3.agentStart = { seq: herdr.commands.at(-1).seq, errorCode: started.errorCode, herdrReportedArgv: started.argv };
      s3.paneArgv = [await recordPaneArgv(ws.paneId, 'after agent start')];
      const proof = s3.paneArgv[0].proof;
      if (!proof.found) ctx.finding(`the pane's process argv could not show a \`codex\` process (${s3.paneArgv[0].argv.map((a) => a.source).join('; ') || 'no processes'}); the plain-launch proof rests on the launch parameter and herdr's reported argv only`);
      else if (!proof.plain) throw new DriverError(`the pane's codex process (pid ${proof.pid}) runs with arguments ${JSON.stringify(proof.argsAfterCodex)}; G2 needs plain \`codex\` with no config overrides`);
      await settle('startup', num('startupTimeoutMs'));
      // #204: the composer Codex 0.159.2 shows at startup is its startup draft, not a session;
      // the operator's message is typed only once the session is verified ready
      // (lib/g2.mjs codexReadiness: a new loaded thread on the wire + the idle composer).
      const ready = await waitCodexReady({
        read,
        handleDialog,
        listLoaded: async () => {
          const { linesBefore } = await runClient('list', []);
          return loadedSince(facts().loadedLists, linesBefore); // read after the poll: its own answer only
        },
        preLoaded: s3.preLaunch.loaded,
        timeoutMs: num('readyTimeoutMs'),
        pollMs: num('listPollMs'),
        remainingMs: () => ctx.remainingMs(),
        stop,
        onTimeout: (v) => {
          const f = codexReadyTimeoutFinding(v);
          if (f) ctx.finding(f);
        },
      });
      s3.codexReady = { readSeq: ready.readSeq, newThreads: ready.newThreads.length, polls: ready.polls, waitedMs: ready.waitedMs, observations: ready.observations };
      if (ready.newThreads.length > 1) ctx.finding(multipleNewThreadsFinding(ready.newThreads.length));
      // #282: the session loaded on the wire is not the end of Codex's startup: settle (idle at
      // or past herdr's state at this observation, still idle on a re-check) before typing.
      s3.codexStartupSettle = await settleAfterObservation({
        herdr, name: AGENT, g: s3, context: 'codex-ready-settle', what: `the Codex session loaded in the daemon (pane read #${ready.readSeq})`,
        timeoutMs: num('turnTimeoutMs'), settleMs: num('settleMs'), ctx, stop, sleep,
        settleTo: async ({ floor, timeoutMs }) => {
          const s = await settle('codex-ready-settle', timeoutMs, { floor });
          return { state: s.state, stateChangeSeq: s.stateChangeSeq, waitSeq: s.waitSeq, readSeq: s.read.seq };
        },
      });

      // --- 4. the operator's own message; find the TUI's thread on the wire ----------------
      // #253 (#282 review): never typed into a running turn; an `agent get` baseline first.
      const promptBase = await refuseRunningTurn({ herdr, name: AGENT, g: s3, context: 'operator-prompt', ctx, stop });
      const res = await herdr.agentPrompt(AGENT, operatorPrompt);
      s3.operatorInput = { seq: res.entry.seq, startedAt: res.entry.startedAt, endedAt: res.entry.endedAt, text: operatorPrompt, baseline: { seq: promptBase.seq, state: promptBase.state, stateChangeSeq: promptBase.stateChangeSeq } };
      const listFrom = lineCount();
      // This wait only schedules the next read: it can return at once, with the state from
      // before the prompt was picked up (#253). The operator's turn being over is established
      // on the wire below (the watch stream's thread/resume status or thread/status/changed
      // idle) before anything is delivered.
      const after = (await waitState('operator-turn', num('turnTimeoutMs'))).state;
      const afterRead = await read('after-operator-turn');
      if (after === 'unknown') ctx.finding(`herdr reported agent state \`unknown\` after the operator's turn (herdr command #${s3.herdrStates.at(-1).seq}); recorded, nothing re-sent (K1 §5 item 5)`);
      if (afterRead.screen.dialog) await handleDialog(afterRead, 'operator-turn');
      const attachDeadline = deadlineFor(num('attachTimeoutMs'));
      const attachWaited0 = humanWaitMs;
      let found = null;
      for (;;) {
        await runClient('list', []);
        found = identifyTuiThread(facts(), { operatorPrompt, projectDirs, sinceLine: listFrom });
        if (found.threadId) break;
        if (found.candidates.length > 1) throw new DriverError(`${found.why}: ${found.candidates.map((c) => c.id).join(', ')}`);
        if (leftUntil(attachDeadline, attachWaited0) <= num('listPollMs')) {
          ctx.finding(`no thread loaded in the daemon matched the operator's prompt and the project directory within ${num('attachTimeoutMs')} ms: the TUI may not have attached to the daemon (G2 criterion 1)`);
          stop(`no loaded thread matched the operator's TUI thread within ${num('attachTimeoutMs')} ms (${found.why}); nothing delivered`);
        }
        await sleep(num('listPollMs'));
        const r = await read('attach-wait', { keep: 'on-change' });
        if (r.screen.dialog) await handleDialog(r, 'attach-wait');
      }
      const threadId = found.threadId;
      s3.thread = { id: threadId, listLine: found.candidates[0].listLine, preLaunchLoaded: (s3.preLaunch.loaded ?? []).includes(threadId) };

      // --- 5. subscribe; the thread must be idle on the event stream ------------------------
      const watchFrom = lineCount();
      watchProc = spawnLongRunning(process.execPath, [join(clientDir, 'client.mjs'), 'watch', threadId], { env: process.env, cwd: clientDir });
      const killWatch = () => killTree(watchProc?.child.pid, 'SIGTERM');
      abortSignal?.addEventListener('abort', killWatch, { once: true });
      s3.watch = { startedAt: new Date().toISOString(), pid: watchProc.child.pid ?? null, linesBefore: watchFrom, resumeLine: null, resumeStatus: null, stoppedAt: null };
      const watchExited = () => (watchProc.child.exitCode !== null || watchProc.child.signalCode !== null ? `the client's watch process exited (${watchProc.child.exitCode ?? watchProc.child.signalCode})` : null);
      const resume = await waitWire('the thread/resume result', (f) => f.resumes.find((x) => x.mode === 'watch' && x.line > watchFrom && x.threadId === threadId) ?? null, num('wireTimeoutMs'), { label: 'watch-wait', bail: watchExited });
      if (resume.bailed) diverge(`\`watch\`: ${resume.bailed} before thread/resume answered`);
      if (resume.error) diverge(`\`watch\`: thread/resume answered with error ${JSON.stringify(resume.error)}`);
      s3.watch.resumeLine = resume.line;
      s3.watch.resumeStatus = resume.status;
      const onWatch = (f, list) => list.filter((x) => x.mode === 'watch' && x.line > resume.line);
      const idle =
        resume.status === 'idle'
          ? { line: resume.line, via: 'thread/resume result' }
          : await waitWire('the thread going idle', (f) => {
              const s = onWatch(f, f.events.statusChanged).find((x) => x.threadId === threadId && x.type === 'idle');
              return s ? { line: s.line, via: 'thread/status/changed' } : null;
            }, num('turnTimeoutMs'), { label: 'idle-wait', bail: watchExited });
      if (idle.bailed) diverge(`\`watch\`: ${idle.bailed}`);
      s3.idle = idle;

      // --- 6a. the idle add: thread/queue/add to the TUI's idle, loaded thread ----------------
      // The daemon's own turn record first: the thread's last turn must have completed, not
      // been interrupted (an interrupted last turn holds a queued message back; that case is
      // the client's `cases`).
      if (caseSet === 'all') {
        const before = await runClient('turns', [threadId]);
        const priorTurns = facts().turnsLists.find((x) => x.mode === 'turns' && x.line > before.linesBefore) ?? null;
        const lastTurn = priorTurns?.turns?.[0] ?? null;
        const refusal = idleAddRefusal(priorTurns);
        if (refusal) stop(refusal);
        once('thread/queue/add to the idle thread (client `idleadd`)');
        const ia = await runClient('idleadd', [threadId, idleAddText]);
        const add = facts().queueAdds.find((x) => x.mode === 'idleadd' && x.line > ia.linesBefore);
        if (!add?.queuedId) diverge(`\`idleadd\`: thread/queue/add answered ${JSON.stringify(add?.error ?? 'no queuedSubmission id')}`);
        const queuedItem = await waitWire('the queued message starting a turn on the idle thread', (f) => onWatch(f, f.events.userItems).find((x) => x.method === 'item/started' && x.clientId === add.clientUserMessageId && x.turnId) ?? null, num('turnTimeoutMs'), { label: 'idle-add-wait', bail: watchExited });
        if (queuedItem.bailed) diverge(`\`watch\`: ${queuedItem.bailed}`);
        const queuedStarted = facts().events.turnStarted.find((x) => x.mode === 'watch' && x.turnId === queuedItem.turnId) ?? null;
        const queuedDone = await waitWire('turn/completed for the idle-add turn', (f) => onWatch(f, f.events.turnCompleted).find((x) => x.turnId === queuedItem.turnId) ?? null, num('turnTimeoutMs'), { label: 'idle-add-turn', bail: watchExited });
        if (queuedDone.bailed) diverge(`\`watch\`: ${queuedDone.bailed}`);
        const idleRead = await settledRead('after-idle-add', { source: 'recent-unwrapped', lines: num('readLines') }, 'idle-add-turn', `turn/completed for the idle-add turn ${queuedItem.turnId} on the watch stream (transcript line ${queuedDone.line})`); // #246
        s3.idleAdd = {
          text: idleAddText,
          priorTurnsLine: priorTurns.line,
          priorLastTurn: { id: lastTurn.id, status: lastTurn.status },
          run: s3.clientRuns.length - 1,
          requestLine: add.reqLine,
          responseLine: add.line,
          clientUserMessageId: add.clientUserMessageId,
          queuedSubmissionId: add.queuedId,
          turnStartedLine: queuedStarted?.line ?? null,
          turnId: queuedItem.turnId,
          userItemLine: queuedItem.line,
          completedLine: queuedDone.line,
          completedStatus: queuedDone.status,
          agentMessages: queuedDone.agentMessages,
          noTurnStartSent: facts().turnStarts.every((x) => x.threadId !== threadId),
          afterReadSeq: idleRead.seq,
        };
      } else {
        s3.idleAdd = { skipped: `case set ${caseSet}: the idle add runs only in the full set` };
      }

      // --- 6b. the cases, on threads the client starts itself -----------------------------------
      once('the cases (client `cases`)');
      const casesDir = ctx.dir('s3-cases-project');
      const cr = await runClient('cases', [casesDir, caseSet, String(num('quietMs'))], { deadlineMs: num('casesTimeoutMs') });
      const observations = [];
      for (const l of cr.stdoutStatus) {
        const m = /^\[case\] (.*)$/.exec(l);
        if (!m) continue;
        try {
          observations.push(JSON.parse(m[1]));
        } catch {
          observations.push({ unparsed: l });
        }
      }
      const ignoredRequests = cr.stdoutStatus.filter((l) => l.startsWith('[server-request ignored'));
      if (ignoredRequests.length) ctx.finding(`the daemon sent the cases client ${ignoredRequests.length} server-to-client request(s), which it never answers: ${ignoredRequests.join('; ')}`);
      s3.cases = { run: s3.clientRuns.length - 1, dir: '<cases project directory, in the scratch directory>', observations, ignoredRequests };

      // --- 6c. the daemon's own turn record of the TUI thread -----------------------------------
      const tl = await runClient('turns', [threadId]);
      s3.turnsList = { run: s3.clientRuns.length - 1, line: facts().turnsLists.find((x) => x.mode === 'turns' && x.line > tl.linesBefore)?.line ?? null };

      // --- 7. post-run versions ----------------------------------------------------------------
      const post = await harnessVersions(['codex']);
      const dv2 = await codexCli(['app-server', 'daemon', 'version'], 'daemon version (post-run)');
      s3.daemon.versionAfter = { ...dv2.rec, parsed: parseCodexDaemonVersion(dv2.r.stdout) };
      const postCli = parseCodexCliVersion(post.codex);
      // "Unchanged through the run" compares with the version the run started on, not with
      // PINS.md: versions float (#216).
      const postDaemonOk = CODEX_DAEMON_VERSION_FIELDS.every((k) => s3.daemon.versionAfter.parsed?.[k] === cli);
      // `cases-preinit` never sends initialize, so it reports no version: it is left out.
      const wireVersions = wireVersionsOf(facts().connections);
      s3.postRun = { cliOutput: post.codex, cli: postCli, daemon: s3.daemon.versionAfter.parsed, wireVersionsSeen: wireVersions, matches: !!cli && postCli === cli && postDaemonOk && wireVersions.length === 1 && wireVersions[0] === cli };
      if (!s3.postRun.matches) {
        ctx.finding(`the Codex version changed during the run or differed between connections (CLI ${cliRaw} before, ${post.codex} after; daemon after ${JSON.stringify(s3.daemon.versionAfter.parsed)}; wire versions ${JSON.stringify(wireVersions)}); the daemon can update itself mid-run (PINS.md "Version policy"). The run is not stopped, but the captures lose their fixture names`);
        // Evidence spanning two Codex versions is not a fixture of either: back to unverified-*.
        s3.versions.verified = false;
        s3.fixtures = null;
        s3.captureNames = s3UnverifiedNames(s3.date, caseSet);
      }
      s3.paneArgv.push(await recordPaneArgv(ws.paneId, 'end of run'));
    } finally {
      if (watchProc) {
        killTree(watchProc.child.pid, 'SIGTERM');
        await within(watchProc.exited, 3000);
        if (s3.watch) s3.watch.stoppedAt = new Date().toISOString();
      }
      // The capture is sanitized structurally (lib/g2.mjs) before run.mjs's own redaction;
      // installation ids are also registered as run literals so no other copy survives.
      let transcriptText = null;
      if (clientDir && existsSync(transcriptPath())) {
        const s = sanitizeTranscript(readFileSync(transcriptPath(), 'utf8'), { ownThreadId: s3.thread?.id ?? null });
        for (const id of s.installationIds) ctx.redactLiteral(id, '<INSTALLATION_ID>');
        s3.sanitizer = { ownThreadId: s3.thread?.id ?? null, ...s.report };
        transcriptText = s.text;
      }
      ctx.record('s3', s3);
      if (s3.captureNames) {
        if (sections.length) ctx.capture(s3.captureNames.pane, sections.join(''));
        if (transcriptText !== null) ctx.capture(s3.captureNames.transcript, transcriptText, { format: 'jsonl' });
      }
    }
  },
};
