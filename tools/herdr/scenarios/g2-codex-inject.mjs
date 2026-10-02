// g2-codex-inject: G2 re-run end to end through the herdr driver (Epic K, K7 #130).
//
// NOT VERDICT-BEARING. This scenario re-runs the human-run G2 `0.157.1` re-run
// (docs/planning/gates/G2-result.md) through herdr and records the run so it can be compared
// against that run's fixture, criterion by criterion (tools/herdr/lib/g2-report.mjs). It
// never changes G2's verdict, STATUS.md, or PINS.md.
//
// LIVE STATUS: UNVERIFIED. This scenario has only been exercised against the test doubles
// in tools/herdr/test/ (a fake herdr and a fake Codex); it has never driven a real herdr or
// a real Codex. Every Codex pane-text pattern it relies on is a guess to be confirmed by the
// first operator run (lib/g2.mjs).
//
// Operator command (a machine with herdr at the PINS.md pin and a Codex CLI, any version:
// versions float, and one other than PINS.md's last tested version is a VERSION WARNING
// finding, never a stop (#216); already signed in the way the operator normally uses it; the
// launch below is the default and may be omitted):
//
//   node tools/herdr/run.mjs --scenario g2-codex-inject --launch '["codex"]' \
//     --out <run dir>
//   node tools/herdr/lib/g2-report.mjs --run <run dir>            # draft comparison
//
// What it does, in the human run's order:
//   0. Preflight. The launch must be plain `codex`: no argument at all, so no config
//      override. `codex --version` is compared with the `Codex CLI / app-server` minimum and
//      last tested versions in PINS.md AS COMMITTED at HEAD (an uncommitted PINS.md edit is
//      a finding). A difference is a VERSION WARNING finding and the run continues: versions
//      float, warn, never gate (#216). The run never edits PINS.md. The quarantined
//      G2 client is staged into the run's scratch directory from its blob committed at
//      HEAD, and the copy's sha256 checked against that blob and against the sha256 the G2
//      runs used; a working-tree file that differs from HEAD (edited, replaced, symlinked)
//      refuses the run. It is never edited or imported: it runs as its own process, exactly
//      as the human run ran it (`node client.mjs <mode> ...`).
//   1. `codex app-server daemon start` (a driver child process, bounded; not typed into a
//      pane), then `codex app-server daemon version`: its cliVersion, appServerVersion and
//      managedCodexVersion are compared with PINS.md too (VERSION WARNING, never a stop).
//      The driver never stops the daemon: it is the operator's, as in the human run.
//   2. The client's `list` before the launch: the wire `initialize` userAgent version is
//      compared too (VERSION WARNING). Captures get the K7 fixture names only when the CLI,
//      all three daemon fields and the wire report one and the same version, so a capture
//      names one version; otherwise they stay `unverified-*` (a finding; the run continues).
//   3. Launch through `herdr agent start --kind codex` with nothing after it. The pane's
//      process argv is read from the OS (never its environment) and recorded as the proof
//      of a plain launch; any argument after `codex` stops the run. Every dialog is read
//      from the pane verbatim BEFORE any keystroke reaches it. accept=driver (the default
//      since #196) accepts only Codex's workspace-trust dialog as recorded on 0.159.2 (#199,
//      lib/g2.mjs CODEX_DIALOG_KINDS: "› 1. Trust and continue" preselected, so `enter`
//      alone); any other Codex dialog, or that one in any other shape, ends the run NOT RUN
//      with no key sent (#197 review). accept=human sends nothing and waits for the
//      operator. No G2 criterion names a consent step.
//   4. The operator's own message to their TUI (`agent prompt`, as the operator typed it
//      in the human run; it is never a delivered message). herdr's agent state after it is
//      recorded and used only to schedule the next step; an `unknown` state is recorded,
//      never retried. The TUI's thread is then found on the wire only: loaded in the daemon
//      (`thread/loaded/list`) and listed with the operator's prompt as its preview and the
//      scratch project directory as its cwd (`thread/list`), by repeated client `list`
//      runs, bounded.
//   5. The client's `watch` subscribes to the thread (`thread/resume`); from then on, turn
//      completion comes only from that event stream. The thread must be idle on the stream
//      before anything is delivered.
//   6. Delivery, each exactly once, only through the second client: `turn` (turn/start into
//      the idle thread), then `busyqueue` (turn/start of a long turn, then thread/queue/add
//      while it runs -- the committed client's own texts), then `turns` (the daemon's own
//      turn record). herdr types none of it. Pane reads are kept while each turn runs.
//   7. Post-run: `codex --version` and the daemon's versions again (the daemon can update
//      itself mid-run; PINS.md "Version policy"); a change is a finding, never a stop.
//   A client that does not work unmodified on the observed version is recorded as a
//   divergence and the run FAILs; the client is never patched to get past it.
//   Captures (redacted by run.mjs after the fixture sanitizer in lib/g2.mjs):
//   `transcript-<date>-<version>-herdr.jsonl` (the client's own wire transcript) and
//   `pane-<date>-<version>-herdr.txt` (every kept pane read, verbatim, each with its herdr
//   command seq and timestamps).
//
// Credentials: nothing here reads a file under the Codex home directory or asks any OS
// credential store for anything; the daemon serves every model turn from its own sign-in.
// tools/herdr/test/g2-tests.mjs traces every file this driver and the client open during a
// fake run and fails if either touches the Codex home beyond the harness-config hashes
// run.mjs records.

import { existsSync, readFileSync, realpathSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { NotRunError, DriverError } from '../lib/herdr.mjs';
import { parseCodexVersions, pinsReadWarning, parseCodexCliVersion, parseCodexDaemonVersion, codexVersionWarning, CODEX_PIN_ROW, CODEX_DAEMON_VERSION_FIELDS } from '../lib/pins.mjs';
import { harnessVersions } from '../lib/manifest.mjs';
import { runBounded, spawnLongRunning, killTree, descendants, processTable, within } from '../lib/proc.mjs';
import { CODEX_DAEMON_SCRATCH_HOLDER } from '../lib/scratch.mjs';
import { committedFile, formatSection, sameDialog, acceptHint } from '../lib/g1.mjs';
import { driverAcceptDialog } from '../lib/gate-common.mjs';
import {
  G2_LAUNCH, COMMITTED_CLIENT, COMMITTED_CLIENT_SHA256, PINS_PATH, DEFAULT_OPERATOR_PROMPT, defaultInjectText, assertNotInjected, stageClientCopy,
  fixtureNames, unverifiedNames, classifyCodexScreen, driverMayAcceptCodex, normalizeDialogText, paneArgv, parseG2Transcript,
  g2Facts, identifyTuiThread, sanitizeTranscript, CODEX_DIALOG_KINDS, waitCodexReady, loadedSince, codexReadyTimeoutFinding, multipleNewThreadsFinding,
} from '../lib/g2.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..', '..');
const AGENT = 'g2codex';
const INPUT_ROLES = new Set(['operator-input', 'dialog-accept']);
const sleep = (ms) => new Promise((r) => setTimeout(r, Math.max(0, ms)));
const cap = (s, n) => (String(s ?? '').length > n ? `${String(s).slice(0, n)}… (${String(s).length} chars)` : String(s ?? ''));

export default {
  name: 'g2-codex-inject',
  description: 'G2 re-run through herdr for comparison with the human-run 0.157.1 re-run (K7). Not verdict-bearing.',
  harnesses: ['codex'],
  defaults: {
    launch: [...G2_LAUNCH],
    timeboxMs: 45 * 60 * 1000, // the 0.157.1 re-run declared 45 minutes (G2-result.md "Timebox")
    params: {
      accept: 'driver', // #196: the driver accepts recognized dialogs by default; accept=human remains
      operatorPrompt: DEFAULT_OPERATOR_PROMPT,
      injectText: '',
      startupTimeoutMs: '120000',
      humanAcceptTimeoutMs: '300000',
      cliTimeoutMs: '60000',
      clientTimeoutMs: '60000',
      wireTimeoutMs: '30000',
      readyTimeoutMs: '120000', // #204: bound on the verified-ready wait before the operator's message
      attachTimeoutMs: '180000',
      listPollMs: '3000',
      turnTimeoutMs: '300000',
      busyQueueTimeoutMs: '240000', // the committed client's busyqueue mode waits up to 150 s itself
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
    const operatorPrompt = params.operatorPrompt || DEFAULT_OPERATOR_PROMPT;
    try {
      assertNotInjected('operatorPrompt', operatorPrompt);
    } catch (err) {
      throw new DriverError(err.message);
    }

    const g2 = {
      nonVerdictBearing: 'K7: compared against the human-run G2 0.157.1 re-run; never changes the G2 verdict',
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
      inject: null,
      busyQueue: null,
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
      g2.stoppedAt = reason;
      throw new NotRunError(reason);
    };
    const diverge = (what) => {
      const msg = `the committed G2 client did not work unmodified on this Codex version: ${what}. Recorded as a divergence; the client is not patched to get past it`;
      g2.divergence.push(what);
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
    const clientProblems = (rec) => {
      const f = facts();
      const mine = f.connections.filter((c) => c.mode === rec.mode && c.firstLine > rec.linesBefore);
      const out = [];
      if (!mine.length) out.push('no connection on the wire (no WebSocket upgrade recorded)');
      for (const c of mine) {
        if (c.upgraded === false) out.push(`WebSocket upgrade refused: ${c.handshake}`);
        if (!c.userAgent) out.push('initialize returned no userAgent');
        if (c.unparsed) out.push(`${c.unparsed} unparsed frame(s)`);
        for (const e of c.errors) out.push(`${e.method} answered with error ${JSON.stringify(e.error)}`);
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
      stdoutStatus: r ? String(r.stdout ?? '').split('\n').filter((l) => /^\[(?:done|error|upgrade failed)\]/.test(l)).map((l) => cap(l, 300)) : [],
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
      g2.clientRuns.push(rec);
      aborted();
      if (r.timedOut) stop(`the client's \`${mode}\` run timed out after ${deadlineMs} ms; nothing re-sent`);
      if (r.spawnError) throw new DriverError(`could not run the staged client (${r.spawnError})`);
      rec.problems = clientProblems(rec).problems;
      if (rec.problems.length) diverge(`\`${mode}\`: ${rec.problems.join('; ')}`);
      return rec;
    };

    // Each delivery happens once, whatever happens next: nothing is ever re-sent.
    const sent = new Set();
    const once = (what) => {
      if (sent.has(what)) throw new DriverError(`refusing to send ${what} a second time; nothing is re-sent`);
      sent.add(what);
      g2.injectionsSent.push({ what, at: new Date().toISOString() });
    };

    // --- dialogs: text on record before any keystroke ------------------------------------
    const handleDialog = async (r, context) => {
      keepSection(r, r.text);
      if (g2.dialogs.length >= num('maxDialogs')) stop(`more than ${num('maxDialogs')} dialogs; stopping`);
      const kind = r.screen.dialog;
      const d = { index: g2.dialogs.length + 1, kind, context, patternVerified: CODEX_DIALOG_KINDS[kind]?.verified ?? null, readSeq: r.seq, readAt: r.startedAt, selected: r.screen.selected, acceptOrigin: null, acceptSeq: null, resolvedSeq: null, inputBetweenReadAndAccept: null };
      g2.dialogs.push(d);
      if (accept === 'driver') {
        await driverAcceptDialog({ herdr, target: AGENT, r, d, kind, dialogKinds: CODEX_DIALOG_KINDS, plan: driverMayAcceptCodex(r.screen), read, stop, num, sleep, deadlineFor });
        return;
      }
      const before = herdr.commands.length;
      console.error(
        `\n[g2-codex-inject] dialog ${d.index} (${kind}) is on screen; its text is recorded (herdr command #${r.seq}).\n` +
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
    const waitState = async (context, timeoutMs) => {
      const w = await herdr.agentWait(AGENT, { until: ['idle', 'done', 'blocked', 'unknown'], timeoutMs: Math.max(1000, timeoutMs) });
      const state = w.json?.result?.agent?.state ?? null;
      g2.herdrStates.push({ context, seq: w.entry.seq, state });
      return state;
    };

    // Wait until the pane shows neither a dialog nor work in progress.
    const settle = async (context, timeoutMs) => {
      await sleep(num('settleMs'));
      const deadline = deadlineFor(timeoutMs);
      const waited0 = humanWaitMs;
      let blockedUnseen = 0;
      for (;;) {
        const left = leftUntil(deadline, waited0);
        if (left <= 0) stop(`${context}: the pane did not settle within ${timeoutMs} ms`);
        const state = await waitState(context, left);
        const r = await read(`${context}-settled?`, { keep: 'on-change' });
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
        return { read: r, state };
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
      const info = await herdr.paneProcessInfo(paneId);
      const fg = (info.foreground_processes ?? []).map((p) => p.pid).filter(Number.isInteger);
      // One process table for the whole tree (#136 review: a table per call costs ~1.6 s on Windows).
      const procTable = processTable();
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
      g2.launch.verbatim = JSON.stringify(launch) === JSON.stringify(G2_LAUNCH);
      if (!g2.launch.verbatim) {
        throw new DriverError(`launch ${JSON.stringify(launch)} is not plain \`codex\` (${JSON.stringify(G2_LAUNCH)}); G2 needs a TUI launched with no config overrides, so this would not be a G2 re-run`);
      }
      const cliRaw = ctx.harnessVersion('codex');
      const pinsFile = committedFile(REPO, PINS_PATH);
      // Versions float and are never gated (#216): every difference from PINS.md's minimum
      // or last tested version is a VERSION WARNING finding, and the run continues.
      const pin = parseCodexVersions(pinsFile.bytes.toString('utf8'));
      const cli = parseCodexCliVersion(cliRaw);
      g2.versions = {
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
        g2.versions.warnings.push(w);
        ctx.finding(w);
      };
      if (!pinsFile.workingTreeMatchesHead) ctx.finding(`${PINS_PATH} has uncommitted changes; the version check read the committed PINS.md (HEAD ${pinsFile.headCommit})`);
      warn(pinsReadWarning(pin, 'G2'));
      if (!cli && /^N\/A/.test(cliRaw ?? 'N/A')) stop(`codex --version could not be run (${cliRaw ?? 'not recorded'}); nothing launched`);
      warn(codexVersionWarning({ observed: cli, lastTested: pin.lastTested, minimum: pin.minimum, source: '`codex --version`' }));
      g2.captureNames = unverifiedNames(g2.date);

      clientDir = ctx.dir('g2-client');
      const staged = stageClientCopy(REPO, COMMITTED_CLIENT, clientDir);
      g2.client = {
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
      if (staged.committedSha256 !== COMMITTED_CLIENT_SHA256) throw new DriverError(`${COMMITTED_CLIENT} at HEAD has sha256 ${staged.committedSha256}, not the ${COMMITTED_CLIENT_SHA256} the G2 runs used; refusing to run`);
      if (!staged.workingTreeMatchesHead) throw new DriverError(`${COMMITTED_CLIENT} in the working tree differs from HEAD (edited, replaced or symlinked); refusing to run`);
      if (!staged.match) throw new DriverError('the staged client copy does not match the committed blob (sha256)');

      const projectDir = ctx.dir('g2-project');
      projectDirs = [...new Set([projectDir, realpathSync(projectDir)])];

      // --- 1. daemon ------------------------------------------------------------------------
      ctx.noteScratchHolder?.(CODEX_DAEMON_SCRATCH_HOLDER);
      const ds = await codexCli(['app-server', 'daemon', 'start'], 'daemon start');
      g2.daemon.start = { ...ds.rec, stdout: cap(ds.r.stdout.trim(), 2000), stderr: cap(ds.r.stderr.trim(), 2000) };
      if (ds.r.spawnError || ds.r.exitCode !== 0) throw new DriverError(`\`codex app-server daemon start\` failed (${ds.r.spawnError ?? `exit ${ds.r.exitCode}`}); nothing launched`);
      const dv = await codexCli(['app-server', 'daemon', 'version'], 'daemon version');
      const daemonV = parseCodexDaemonVersion(dv.r.stdout);
      g2.daemon.versionBefore = { ...dv.rec, parsed: daemonV };
      g2.versions.daemon = daemonV;
      for (const k of CODEX_DAEMON_VERSION_FIELDS) {
        warn(codexVersionWarning({ observed: daemonV?.[k] ?? null, lastTested: pin.lastTested, minimum: pin.minimum, source: `\`codex app-server daemon version\` ${k}` }));
      }

      // --- 2. pre-launch list; the wire version --------------------------------------------
      const pre = await runClient('list', []);
      const preConn = clientProblems(pre).connections[0];
      const preFacts = facts();
      g2.preLaunch = { run: g2.clientRuns.length - 1, loaded: loadedSince(preFacts.loadedLists, pre.linesBefore) };
      // #205 review: the ready wait needs this baseline; without it nothing is launched.
      if (g2.preLaunch.loaded === null) stop('the pre-launch `thread/loaded/list` could not be read, so the ready wait (#204) could not tell a thread new since the launch; nothing launched');
      g2.versions.wireUserAgent = preConn.userAgent;
      g2.versions.wire = preConn.userAgentVersion;
      warn(codexVersionWarning({ observed: preConn.userAgentVersion, lastTested: pin.lastTested, minimum: pin.minimum, source: 'the wire initialize userAgent' }));
      const sameVersion = !!cli && preConn.userAgentVersion === cli && CODEX_DAEMON_VERSION_FIELDS.every((k) => daemonV?.[k] === cli);
      if (sameVersion) {
        g2.versions.verified = true; // CLI, daemon and wire report one and the same version
        g2.versions.matchesLastTested = cli === pin.lastTested;
        g2.fixtures = fixtureNames(g2.date, cli);
        g2.captureNames = g2.fixtures;
      } else {
        ctx.finding(`the Codex CLI (${cliRaw}), daemon (${JSON.stringify(daemonV)}) and wire (${preConn.userAgentVersion ?? 'none'}) do not report one and the same version; the run continues, but its captures stay unverified-* because they cannot name one Codex version`);
      }
      const injectText = params.injectText || defaultInjectText(cli ?? 'unknown');
      g2.params.injectText = injectText;

      // --- 3. launch plain codex; the pane's argv; dialogs -----------------------------------
      const ws = await herdr.workspaceCreate({ cwd: projectDir, label: 'oac-g2' });
      ctx.record('workspace', ws);
      await herdr.paneProcessInfo(ws.paneId);
      await ctx.probeEnv(ws.paneId);
      const started = await ctx.startAgent(AGENT, { paneId: ws.paneId, timeoutMs: num('startupTimeoutMs'), allowErrorCodes: ['agent_not_ready'] });
      g2.agentStart = { seq: herdr.commands.at(-1).seq, errorCode: started.errorCode, herdrReportedArgv: started.argv };
      g2.paneArgv = [await recordPaneArgv(ws.paneId, 'after agent start')];
      const proof = g2.paneArgv[0].proof;
      if (!proof.found) ctx.finding(`the pane's process argv could not show a \`codex\` process (${g2.paneArgv[0].argv.map((a) => a.source).join('; ') || 'no processes'}); the plain-launch proof rests on the launch parameter and herdr's reported argv only`);
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
        preLoaded: g2.preLaunch.loaded,
        timeoutMs: num('readyTimeoutMs'),
        pollMs: num('listPollMs'),
        remainingMs: () => ctx.remainingMs(),
        stop,
        onTimeout: (v) => {
          const f = codexReadyTimeoutFinding(v);
          if (f) ctx.finding(f);
        },
      });
      g2.codexReady = { readSeq: ready.readSeq, newThreads: ready.newThreads.length, polls: ready.polls, waitedMs: ready.waitedMs, observations: ready.observations };
      if (ready.newThreads.length > 1) ctx.finding(multipleNewThreadsFinding(ready.newThreads.length));

      // --- 4. the operator's own message; find the TUI's thread on the wire ----------------
      const res = await herdr.agentPrompt(AGENT, operatorPrompt);
      g2.operatorInput = { seq: res.entry.seq, startedAt: res.entry.startedAt, endedAt: res.entry.endedAt, text: operatorPrompt };
      const listFrom = lineCount();
      const after = await waitState('operator-turn', num('turnTimeoutMs'));
      const afterRead = await read('after-operator-turn');
      if (after === 'unknown') ctx.finding(`herdr reported agent state \`unknown\` after the operator's turn (herdr command #${g2.herdrStates.at(-1).seq}); recorded, nothing re-sent (K1 §5 item 5)`);
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
      g2.thread = { id: threadId, listLine: found.candidates[0].listLine, preLaunchLoaded: (g2.preLaunch.loaded ?? []).includes(threadId) };

      // --- 5. subscribe; the thread must be idle on the event stream ------------------------
      const watchFrom = lineCount();
      watchProc = spawnLongRunning(process.execPath, [join(clientDir, 'client.mjs'), 'watch', threadId], { env: process.env, cwd: clientDir });
      const killWatch = () => killTree(watchProc?.child.pid, 'SIGTERM');
      abortSignal?.addEventListener('abort', killWatch, { once: true });
      g2.watch = { startedAt: new Date().toISOString(), pid: watchProc.child.pid ?? null, linesBefore: watchFrom, resumeLine: null, resumeStatus: null, stoppedAt: null };
      const watchExited = () => (watchProc.child.exitCode !== null || watchProc.child.signalCode !== null ? `the client's watch process exited (${watchProc.child.exitCode ?? watchProc.child.signalCode})` : null);
      const resume = await waitWire('the thread/resume result', (f) => f.resumes.find((x) => x.mode === 'watch' && x.line > watchFrom && x.threadId === threadId) ?? null, num('wireTimeoutMs'), { label: 'watch-wait', bail: watchExited });
      if (resume.bailed) diverge(`\`watch\`: ${resume.bailed} before thread/resume answered`);
      if (resume.error) diverge(`\`watch\`: thread/resume answered with error ${JSON.stringify(resume.error)}`);
      g2.watch.resumeLine = resume.line;
      g2.watch.resumeStatus = resume.status;
      const onWatch = (f, list) => list.filter((x) => x.mode === 'watch' && x.line > resume.line);
      const idle =
        resume.status === 'idle'
          ? { line: resume.line, via: 'thread/resume result' }
          : await waitWire('the thread going idle', (f) => {
              const s = onWatch(f, f.events.statusChanged).find((x) => x.threadId === threadId && x.type === 'idle');
              return s ? { line: s.line, via: 'thread/status/changed' } : null;
            }, num('turnTimeoutMs'), { label: 'idle-wait', bail: watchExited });
      if (idle.bailed) diverge(`\`watch\`: ${idle.bailed}`);
      g2.idle = idle;

      // --- 6a. injection 1: turn/start into the idle thread ---------------------------------
      once('turn/start (client `turn`)');
      const ti = await runClient('turn', [threadId, injectText]);
      const injTurn = facts().turnStarts.find((x) => x.mode === 'turn' && x.line > ti.linesBefore);
      if (!injTurn?.turnId) diverge('`turn`: turn/start returned no turn id');
      const injDone = await waitWire('turn/completed for the delivered turn', (f) => onWatch(f, f.events.turnCompleted).find((x) => x.turnId === injTurn.turnId) ?? null, num('turnTimeoutMs'), { label: 'inject-turn', bail: watchExited });
      if (injDone.bailed) diverge(`\`watch\`: ${injDone.bailed}`);
      const injRead = await read('after-inject', { source: 'recent-unwrapped', lines: num('readLines') });
      g2.inject = { text: injectText, run: g2.clientRuns.length - 1, turnId: injTurn.turnId, startLine: injTurn.reqLine, completedLine: injDone.line, completedStatus: injDone.status, agentMessages: injDone.agentMessages, afterReadSeq: injRead.seq };

      // --- 6b. injections 2 and 3: a long turn, then thread/queue/add while it runs --------
      once('turn/start + thread/queue/add (client `busyqueue`)');
      const bqBefore = lineCount();
      let bqResult = null;
      const bqPromise = runBounded(process.execPath, [join(clientDir, 'client.mjs'), 'busyqueue', threadId], { deadlineMs: Math.min(num('busyQueueTimeoutMs'), Math.max(1, ctx.remainingMs())), env: process.env, cwd: clientDir, abortSignal }).then((r) => (bqResult = r));
      const bqExitedEarly = () => (bqResult && (bqResult.exitCode !== 0 || bqResult.timedOut) ? `the client's busyqueue process ended (${bqResult.timedOut ? 'timed out' : `exit ${bqResult.exitCode}`})` : watchExited());
      const bqStart = await waitWire('the busy turn/start result', (f) => f.turnStarts.find((x) => x.mode === 'busyqueue' && x.line > bqBefore) ?? null, num('wireTimeoutMs'), { label: 'busy-start', bail: bqExitedEarly });
      if (bqStart.bailed) diverge(`\`busyqueue\`: ${bqStart.bailed} before turn/start answered`);
      if (bqStart.error || !bqStart.turnId) diverge(`\`busyqueue\`: turn/start answered ${JSON.stringify(bqStart.error ?? 'no turn id')}`);
      const qa = await waitWire('the thread/queue/add result', (f) => f.queueAdds.find((x) => x.mode === 'busyqueue' && x.line > bqBefore) ?? null, num('wireTimeoutMs') + 5000, { label: 'busy-poll', bail: bqExitedEarly });
      if (qa.bailed) diverge(`\`busyqueue\`: ${qa.bailed} before thread/queue/add answered`);
      if (qa.error || !qa.queuedId) diverge(`\`busyqueue\`: thread/queue/add answered ${JSON.stringify(qa.error ?? 'no queuedSubmission id')}`);
      const busyDone = await waitWire('turn/completed for the busy turn', (f) => onWatch(f, f.events.turnCompleted).find((x) => x.turnId === bqStart.turnId) ?? null, num('turnTimeoutMs'), { label: 'busy-poll', bail: watchExited });
      if (busyDone.bailed) diverge(`\`watch\`: ${busyDone.bailed}`);
      const queuedTurn = await waitWire('the queued message starting its own turn', (f) => onWatch(f, f.events.userItems).find((x) => x.clientId && x.clientId === qa.clientUserMessageId && x.turnId) ?? null, num('turnTimeoutMs'), { label: 'queued-poll', bail: watchExited });
      if (queuedTurn.bailed) diverge(`\`watch\`: ${queuedTurn.bailed}`);
      const queuedDone = await waitWire('turn/completed for the queued turn', (f) => onWatch(f, f.events.turnCompleted).find((x) => x.turnId === queuedTurn.turnId) ?? null, num('turnTimeoutMs'), { label: 'queued-poll', bail: watchExited });
      if (queuedDone.bailed) diverge(`\`watch\`: ${queuedDone.bailed}`);
      await within(bqPromise, Math.max(1, Math.min(num('busyQueueTimeoutMs'), ctx.remainingMs())));
      aborted();
      const bqRec = clientRecord('busyqueue', [threadId], bqResult, bqBefore);
      bqRec.linesAfter = lineCount();
      g2.clientRuns.push(bqRec);
      if (!bqResult || bqResult.timedOut) stop(`the client's \`busyqueue\` run did not end within ${num('busyQueueTimeoutMs')} ms; nothing re-sent`);
      bqRec.problems = clientProblems(bqRec).problems;
      if (bqRec.problems.length) diverge(`\`busyqueue\`: ${bqRec.problems.join('; ')}`);
      const busyRead = await read('after-busy-and-queued', { source: 'recent-unwrapped', lines: num('readLines') });
      g2.busyQueue = {
        run: g2.clientRuns.length - 1,
        busyTurnId: bqStart.turnId,
        busyStartLine: bqStart.reqLine,
        queueAddLine: qa.reqLine,
        clientUserMessageId: qa.clientUserMessageId,
        queuedSubmissionId: qa.queuedId,
        busyCompletedLine: busyDone.line,
        queuedTurnId: queuedTurn.turnId,
        queuedUserItemLine: queuedTurn.line,
        queuedCompletedLine: queuedDone.line,
        queuedAgentMessages: queuedDone.agentMessages,
        afterReadSeq: busyRead.seq,
      };

      // --- 6c. the daemon's own turn record -----------------------------------------------------
      const tl = await runClient('turns', [threadId]);
      g2.turnsList = { run: g2.clientRuns.length - 1, line: facts().turnsLists.find((x) => x.mode === 'turns' && x.line > tl.linesBefore)?.line ?? null };

      // --- 7. post-run versions ----------------------------------------------------------------
      const post = await harnessVersions(['codex']);
      const dv2 = await codexCli(['app-server', 'daemon', 'version'], 'daemon version (post-run)');
      g2.daemon.versionAfter = { ...dv2.rec, parsed: parseCodexDaemonVersion(dv2.r.stdout) };
      const postCli = parseCodexCliVersion(post.codex);
      // "Unchanged through the run" compares with the version the run started on, not with
      // PINS.md: versions float (#216).
      const postDaemonOk = CODEX_DAEMON_VERSION_FIELDS.every((k) => g2.daemon.versionAfter.parsed?.[k] === cli);
      const wireVersions = [...new Set(facts().connections.map((c) => c.userAgentVersion))];
      g2.postRun = { cliOutput: post.codex, cli: postCli, daemon: g2.daemon.versionAfter.parsed, wireVersionsSeen: wireVersions, matches: !!cli && postCli === cli && postDaemonOk && wireVersions.length === 1 && wireVersions[0] === cli };
      if (!g2.postRun.matches) {
        ctx.finding(`the Codex version changed during the run or differed between connections (CLI ${cliRaw} before, ${post.codex} after; daemon after ${JSON.stringify(g2.daemon.versionAfter.parsed)}; wire versions ${JSON.stringify(wireVersions)}); the daemon can update itself mid-run (PINS.md "Version policy"). The run is not stopped, but the captures lose their fixture names`);
        // Evidence spanning two Codex versions is not a fixture of either: back to unverified-*.
        g2.versions.verified = false;
        g2.fixtures = null;
        g2.captureNames = unverifiedNames(g2.date);
      }
      g2.paneArgv.push(await recordPaneArgv(ws.paneId, 'end of run'));
    } finally {
      if (watchProc) {
        killTree(watchProc.child.pid, 'SIGTERM');
        await within(watchProc.exited, 3000);
        if (g2.watch) g2.watch.stoppedAt = new Date().toISOString();
      }
      // The capture is sanitized structurally (lib/g2.mjs) before run.mjs's own redaction;
      // installation ids are also registered as run literals so no other copy survives.
      let transcriptText = null;
      if (clientDir && existsSync(transcriptPath())) {
        const s = sanitizeTranscript(readFileSync(transcriptPath(), 'utf8'), { ownThreadId: g2.thread?.id ?? null });
        for (const id of s.installationIds) ctx.redactLiteral(id, '<INSTALLATION_ID>');
        g2.sanitizer = { ownThreadId: g2.thread?.id ?? null, ...s.report };
        transcriptText = s.text;
      }
      ctx.record('g2', g2);
      if (g2.captureNames) {
        if (sections.length) ctx.capture(g2.captureNames.pane, sections.join(''));
        if (transcriptText !== null) ctx.capture(g2.captureNames.transcript, transcriptText, { format: 'jsonl' });
      }
    }
  },
};
