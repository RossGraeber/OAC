// g1-claude-wake: G1 re-run end to end through the herdr driver (Epic K, K4 #127).
//
// NOT VERDICT-BEARING. This scenario re-runs every probe of the human-run G1 Box C
// (docs/planning/gates/G1-result.md) through herdr and records the run so it can be compared
// against Box C, criterion by criterion (tools/herdr/lib/g1-report.mjs). It never changes
// G1's verdict, STATUS.md, or PINS.md.
//
// LIVE STATUS: UNVERIFIED. This scenario has only been exercised against the test doubles
// in tools/herdr/test/ (a fake herdr and a fake Claude Code); it has never driven a real
// herdr or a real Claude Code. Every Claude Code pane-text pattern it relies on except the
// dev-channels dialog is a guess to be confirmed by the first operator run (lib/g1.mjs).
//
// Operator command (a machine with herdr at the PINS.md pin and a logged-in Claude Code at
// PINS.md's last-observed version; the launch below is the default and may be omitted):
//
//   node tools/herdr/run.mjs --scenario g1-claude-wake \
//     --launch '["claude","--dangerously-load-development-channels","server:g1spike"]' \
//     --param accept=human --out <run dir>
//   node tools/herdr/lib/g1-report.mjs --run <run dir>            # draft comparison
//
// What it does, in Box C's order:
//   0. Preflight. The launch must be G1's, verbatim. `claude --version` must equal PINS.md's
//      `Claude Code (Channels)` last-observed version, or the run stops NOT RUN with a
//      pin-move trigger (no PINS.md edit). The committed quarantined channel server is copied
//      byte for byte into the run's scratch directory and the copy's sha256 checked against
//      the committed file; it is never edited and never imported. A project `.mcp.json` in
//      a scratch project directory registers the copy as `g1spike` (the way the G1 operator
//      registered it); no harness config is written.
//   1. Launch through `herdr agent start --kind claude -- <launch[1..]>`. Every dialog is
//      read from the pane verbatim BEFORE any keystroke reaches it. With --param
//      accept=human (default) the driver sends nothing and waits for the operator to accept;
//      with accept=driver it accepts only a dialog it recognizes, only when the dialog's own
//      preselected option is the accepting one. The accept origin is recorded. A
//      driver-sent accept is never scored as meeting G1 criterion 5 (lib/g1-report.mjs).
//   2. Wait for the channel-server handshake on the wire; the wire clientInfo.version must
//      equal the CLI version (else pin-move trigger, NOT RUN).
//   3. Idle wake: `wake.trigger` is touched (the channel server's own trigger -- herdr never
//      types a notification) while the pane shows no work in progress.
//   4. The `<channel>` attribute query and the non-identifier-safe `meta` key question, as
//      an operator prompt.
//   5. A busy turn (four sequential foreground sleeps, as Box C's four Start-Sleep calls)
//      with two mid-turn `midturn.trigger` touches ~1.85 s apart, as Box C. Mid-turn timing
//      is established afterwards from the wire timestamps against timestamped pane reads
//      (lib/g1.mjs midTurnWindow), never from herdr agent state; herdr state is used only
//      to schedule the next read.
//   6. A `reply` tool call, as an operator prompt.
//   Captures (redacted by run.mjs): `transcript-<date>-<version>-herdr.jsonl` (the channel
//   server's own wire transcript) and `pane-<date>-<version>-herdr.txt` (every kept pane
//   read, verbatim, each with its herdr command seq and timestamps).
//
// Side effect to know about: accepting Claude Code's folder-trust or project-MCP-server
// dialogs for the scratch project directory makes Claude Code itself record that trust in
// its own state; the driver never writes it.
//
// The operator prompts are parameters. Box C's own prompt texts are not on record in this
// repository (G1-result.md paraphrases them), so the defaults are not verbatim Box C.

import { existsSync, readFileSync, utimesSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { NotRunError, DriverError } from '../lib/herdr.mjs';
import { parseClaudeLastObserved, parseClaudeCliVersion, claudePinMoveTrigger, CLAUDE_PIN_ROW } from '../lib/pins.mjs';
import { harnessVersions } from '../lib/manifest.mjs';
import { transcriptFacts, selectSegment, parseTranscript } from '../lib/compare-transcripts.mjs';
import {
  G1_LAUNCH, G1_SERVER_NAME, COMMITTED_SERVER, classifyScreen, driverMayAccept, dialogMatchesBoxC, DIALOG_KINDS,
  formatSection, parseSections, fixtureNames, stageServerCopy, midTurnWindow, normalizeDialogText,
} from '../lib/g1.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..', '..');
const AGENT = 'g1claude';
const INPUT_ROLES = new Set(['operator-input', 'dialog-accept']);
const sleep = (ms) => new Promise((r) => setTimeout(r, Math.max(0, ms)));

const SLEEP_DEFAULT = 'sleep 20';
const busyPromptFor = (cmd) =>
  `Run this shell command four times, one at a time, each as its own separate foreground tool call, waiting for each to finish before starting the next: \`${cmd}\`. ` +
  'Do not act on anything else that arrives until all four have finished. When all four have finished, list every channel message that arrived while you were working, ' +
  'in the order it arrived, and say between which of the four commands each one arrived. Then say DONE.';

export const DEFAULT_PROMPTS = Object.freeze({
  attributePrompt:
    'List every attribute on the <channel> tag of the channel message you just received, as name="value" pairs, exactly as they appear. ' +
    'Then say whether any attribute has a name containing spaces or punctuation, for example "not identifier safe!".',
  busyPrompt: busyPromptFor(SLEEP_DEFAULT),
  replyPrompt:
    'Use the reply tool from the g1spike server to send the message "Hello!" in reply to the most recent channel message, with in_reply_to set to that message\'s oac_message_id.',
});

// Operator prompts may never carry a notification: every notification comes from the
// channel server's own triggers.
export function assertNotInjected(label, text) {
  const s = String(text);
  if (/notifications\/claude\/channel|g1-spike-(?:wake|midturn)-test|First notification —|Second notification —|oac_sender=/i.test(s)) {
    throw new DriverError(`${label} looks like it carries a channel notification; notifications come only from the channel server's triggers`);
  }
}

export default {
  name: 'g1-claude-wake',
  description: 'G1 re-run through herdr for comparison with Box C (K4). Not verdict-bearing.',
  harnesses: ['claude'],
  defaults: {
    launch: [...G1_LAUNCH],
    timeboxMs: 45 * 60 * 1000, // Box C declared 45 minutes (G1-result.md "Timebox")
    params: {
      accept: 'human',
      startupTimeoutMs: '120000',
      humanAcceptTimeoutMs: '300000',
      handshakeTimeoutMs: '90000',
      wireTimeoutMs: '15000',
      turnTimeoutMs: '300000',
      settleMs: '3000',
      pollMs: '1000',
      midturnDelayMs: '5000',
      midturnGapMs: '1850',
      busyIndicator: 'esc to interrupt',
      readLines: '2000',
      maxDialogs: '12',
      sleepCommand: SLEEP_DEFAULT,
      attributePrompt: DEFAULT_PROMPTS.attributePrompt,
      busyPrompt: '',
      replyPrompt: DEFAULT_PROMPTS.replyPrompt,
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
    const readLines = num('readLines');
    const prompts = {
      attributePrompt: params.attributePrompt,
      busyPrompt: params.busyPrompt || busyPromptFor(params.sleepCommand || SLEEP_DEFAULT),
      replyPrompt: params.replyPrompt,
    };
    for (const [k, v] of Object.entries(prompts)) assertNotInjected(k, v);

    const g1 = {
      nonVerdictBearing: 'K4 proof of concept: compared against G1 Box C; never changes the G1 verdict',
      acceptPolicy: accept,
      criterion5Rule: 'A driver-sent accept of the dev-channels dialog is never scored as meeting G1 criterion 5 (K4; K5 states the rule from a recorded operator decision).',
      params: { ...params, ...prompts },
      launch: { expected: [...G1_LAUNCH], actual: launch, verbatim: null },
      versions: null,
      server: null,
      mcpJson: null,
      date: new Date().toISOString().slice(0, 10),
      fixtures: null,
      agentStart: null,
      dialogs: [],
      devChannelsDialogSeen: false,
      handshake: null,
      triggers: [],
      wake: null,
      attributeQuery: null,
      busyTurn: null,
      reply: null,
      postRunVersion: null,
      stoppedAt: null,
    };
    const sections = [];
    let serverDir = null;
    let lastKeptKey = null;

    const stop = (reason) => {
      herdr.inputHalted ??= reason;
      g1.stoppedAt = reason;
      throw new NotRunError(reason);
    };

    // Verbatim pane read, recorded with the herdr command that made it. keep: true keeps it
    // in the pane capture; 'on-change' keeps it only when the screen changed since the last
    // kept read (polls).
    const read = async (label, { source = 'visible', lines, keep = true } = {}) => {
      const text = await herdr.agentRead(AGENT, { source, lines, deadlineMs: 15000 });
      const e = herdr.commands.at(-1);
      const sec = { seq: e.seq, label, source, startedAt: e.startedAt, endedAt: e.endedAt };
      const key = `${source}\n${text}`;
      if (keep === true || key !== lastKeptKey) keepSection(sec, text);
      return { ...sec, text, screen: classifyScreen(text, { busyIndicator }) };
    };
    const keptSeqs = new Set();
    const keepSection = (sec, text) => {
      if (keptSeqs.has(sec.seq)) return;
      sections.push(formatSection(sec, text));
      keptSeqs.add(sec.seq);
      lastKeptKey = `${sec.source}\n${text}`;
    };

    const transcriptPath = () => join(serverDir, 'transcript.jsonl');
    const wire = () => {
      if (!serverDir || !existsSync(transcriptPath())) return [];
      const text = readFileSync(transcriptPath(), 'utf8');
      return parseTranscript(text.slice(0, text.lastIndexOf('\n') + 1)); // complete lines only
    };
    const lastInstance = () => selectSegment(wire(), 'last');

    const deadlineFor = (ms) => Date.now() + Math.min(ms, Math.max(0, ctx.remainingMs()));

    // --- dialogs: text on record before any keystroke -----------------------------------
    const handleDialog = async (r, context) => {
      keepSection(r, r.text); // a dialog's read is always in the pane record, verbatim
      if (g1.dialogs.length >= num('maxDialogs')) stop(`more than ${num('maxDialogs')} dialogs; stopping`);
      const kind = r.screen.dialog;
      const d = {
        index: g1.dialogs.length + 1,
        kind,
        context,
        patternVerified: DIALOG_KINDS[kind]?.verified ?? null,
        readSeq: r.seq,
        readAt: r.startedAt,
        selected: r.screen.selected,
        matchesBoxC: kind === 'dev-channels' ? dialogMatchesBoxC(r.text) : null,
        acceptOrigin: null,
        acceptSeq: null,
        resolvedSeq: null,
        inputBetweenReadAndAccept: null,
      };
      if (kind === 'dev-channels') g1.devChannelsDialogSeen = true;
      g1.dialogs.push(d);
      if (accept === 'driver') {
        const may = driverMayAccept(r.screen);
        if (!may.ok) {
          d.acceptOrigin = 'none (driver refused)';
          stop(`dialog ${d.index} (${kind}): ${may.why}; the driver did not accept it`);
        }
        const res = await herdr.dialogAccept(AGENT, ['enter']); // refused unless the last command on AGENT was this read
        d.acceptOrigin = 'driver';
        d.acceptSeq = res.entry.seq;
        d.acceptAt = res.entry.startedAt;
        d.inputBetweenReadAndAccept = herdr.commands.filter((c) => c.seq > r.seq && c.seq < res.entry.seq && INPUT_ROLES.has(c.role)).length;
        return;
      }
      // Human accept: the driver sends nothing; it waits for the dialog to change.
      const before = herdr.commands.length;
      console.error(
        `\n[g1-claude-wake] dialog ${d.index} (${kind}) is on screen; its text is recorded (herdr command #${r.seq}).\n` +
          `  Accept it yourself, e.g. \`herdr --session ${ctx.sessionName} agent send-keys ${AGENT} enter\`, or attach and press Enter.\n` +
          `  The driver sends no keystroke to it and waits up to ${num('humanAcceptTimeoutMs')} ms.\n`,
      );
      const was = normalizeDialogText(r.text);
      const deadline = deadlineFor(num('humanAcceptTimeoutMs'));
      for (;;) {
        if (Date.now() >= deadline) stop(`dialog ${d.index} (${kind}) was not accepted by the operator within ${num('humanAcceptTimeoutMs')} ms`);
        await sleep(pollMs);
        const p = await read(`dialog-${d.index}-waiting`, { keep: 'on-change' });
        if (p.screen.dialog !== kind || normalizeDialogText(p.text) !== was) {
          d.acceptOrigin = 'human';
          d.resolvedSeq = p.seq;
          d.inputBetweenReadAndAccept = herdr.commands.slice(before).filter((c) => INPUT_ROLES.has(c.role)).length;
          return;
        }
      }
    };

    // Wait until the pane shows neither a dialog nor work in progress. herdr agent state
    // only schedules the next read; the read decides.
    const settle = async (context, timeoutMs) => {
      await sleep(num('settleMs'));
      const deadline = deadlineFor(timeoutMs);
      let blockedUnseen = 0;
      for (;;) {
        const left = deadline - Date.now();
        if (left <= 0) stop(`${context}: the pane did not settle within ${timeoutMs} ms`);
        const w = await herdr.agentWait(AGENT, { until: ['idle', 'done', 'blocked'], timeoutMs: Math.max(1000, left) });
        const r = await read(`${context}-settled?`, { keep: 'on-change' });
        // herdr saying `blocked` while the screen shows no dialog the driver can name: herdr
        // state may lag the screen, so look again; if it persists, treat it as an
        // unrecognized dialog -- the conservative reading (never accepted by the driver).
        if (!r.screen.dialog && w.json?.result?.agent?.state === 'blocked') {
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
        return r;
      }
    };

    // Wait for a frame on the wire; with watchPane, keep reading the pane (handling any
    // dialog, e.g. a tool permission prompt) every pollMs while waiting.
    const waitWire = async (what, pred, timeoutMs, { watchPane = true, keep = 'on-change', label = what } = {}) => {
      const deadline = deadlineFor(timeoutMs);
      let nextRead = 0;
      for (;;) {
        const hit = pred(lastInstance());
        if (hit) return hit;
        if (Date.now() >= deadline) stop(`timed out after ${timeoutMs} ms waiting for ${what} on the wire`);
        if (watchPane && Date.now() >= nextRead) {
          const r = await read(label, { keep });
          if (r.screen.dialog) await handleDialog(r, what);
          nextRead = Date.now() + pollMs;
        }
        await sleep(200);
      }
    };

    const fire = (kind) => {
      const file = kind === 'wake' ? 'wake.trigger' : 'midturn.trigger';
      const p = join(serverDir, file);
      const now = new Date();
      if (existsSync(p)) utimesSync(p, now, now);
      else writeFileSync(p, '');
      const rec = { kind, file, at: now.toISOString() };
      g1.triggers.push(rec);
      return rec;
    };
    const channelFrames = (kind) => (entries) => transcriptFacts(entries).channelNotifications.filter((n) => n.kind === kind);

    const prompt = async (label, text) => {
      assertNotInjected(label, text);
      const res = await herdr.agentPrompt(AGENT, text);
      return { seq: res.entry.seq, startedAt: res.entry.startedAt, endedAt: res.entry.endedAt };
    };

    try {
      // --- 0. preflight -------------------------------------------------------------------
      g1.launch.verbatim = JSON.stringify(launch) === JSON.stringify(G1_LAUNCH);
      if (!g1.launch.verbatim) {
        throw new DriverError(`launch ${JSON.stringify(launch)} is not G1's verbatim launch ${JSON.stringify(G1_LAUNCH)}; this would not be a G1 re-run`);
      }
      const cliRaw = ctx.harnessVersion('claude');
      const pin = parseClaudeLastObserved(readFileSync(join(REPO, 'docs', 'planning', 'PINS.md'), 'utf8'));
      const cli = parseClaudeCliVersion(cliRaw);
      g1.versions = { pinsRow: CLAUDE_PIN_ROW, pinsLastObserved: pin.lastObserved, cliOutput: cliRaw, cli, wireClientInfo: null };
      if (!cli && /^N\/A/.test(cliRaw ?? 'N/A')) stop(`claude --version could not be run (${cliRaw ?? 'not recorded'}); nothing launched`);
      const trigger = claudePinMoveTrigger({ observed: cli, lastObserved: pin.lastObserved, source: '`claude --version`' });
      if (trigger) {
        ctx.finding(trigger);
        stop(trigger);
      }
      g1.fixtures = fixtureNames(g1.date, cli);

      serverDir = ctx.dir('g1-server');
      const staged = stageServerCopy(join(REPO, COMMITTED_SERVER), serverDir);
      g1.server = { committed: COMMITTED_SERVER, committedSha256: staged.committedSha256, copy: staged.copyPath, copySha256: staged.copySha256, match: staged.match };
      if (!staged.match) throw new DriverError('the staged channel-server copy does not match the committed file (sha256)');

      const projectDir = ctx.dir('g1-project');
      const mcp = { mcpServers: { [G1_SERVER_NAME]: { command: process.execPath, args: [staged.copyPath] } } };
      writeFileSync(join(projectDir, '.mcp.json'), `${JSON.stringify(mcp, null, 2)}\n`);
      g1.mcpJson = { path: join(projectDir, '.mcp.json'), contents: mcp };

      // --- 1. launch; dialogs -----------------------------------------------------------
      const ws = await herdr.workspaceCreate({ cwd: projectDir, label: 'oac-g1' });
      ctx.record('workspace', ws);
      await herdr.paneProcessInfo(ws.paneId);
      await ctx.probeEnv(ws.paneId);
      const started = await ctx.startAgent(AGENT, { paneId: ws.paneId, timeoutMs: num('startupTimeoutMs'), allowErrorCodes: ['agent_not_ready'] });
      g1.agentStart = { seq: herdr.commands.at(-1).seq, errorCode: started.errorCode, herdrReportedArgv: started.argv };
      await herdr.paneProcessInfo(ws.paneId);
      await settle('startup', num('startupTimeoutMs'));

      // --- 2. handshake -------------------------------------------------------------------
      const hs = await waitWire(
        'the channel-server handshake',
        (es) => {
          const f = transcriptFacts(es);
          return f.initialize && f.initializedNotification && f.toolsListed.includes('reply') ? f : null;
        },
        num('handshakeTimeoutMs'),
        { label: 'handshake-wait' },
      );
      g1.handshake = {
        protocolVersion: hs.initialize.protocolVersion,
        negotiated: hs.negotiatedProtocolVersion,
        serverDeclaresChannel: hs.serverDeclaresChannel,
        discoverProbe: hs.discoverProbe,
        serverInstances: transcriptFacts(wire()).serverInstances,
      };
      g1.versions.wireClientInfo = hs.initialize.clientInfo.version;
      const wireTrigger = claudePinMoveTrigger({ observed: hs.initialize.clientInfo.version, lastObserved: pin.lastObserved, source: 'the wire initialize clientInfo.version' });
      if (wireTrigger) {
        ctx.finding(wireTrigger);
        stop(wireTrigger);
      }
      await herdr.paneProcessInfo(ws.paneId);
      await settle('post-handshake', num('startupTimeoutMs'));
      if (!g1.devChannelsDialogSeen) ctx.finding('the dev-channels confirmation dialog was never recognized on screen before the session settled (G1 criterion 5)');

      // --- 3. idle wake -------------------------------------------------------------------
      const pre = await read('pre-wake-idle-check');
      const promptsBefore = herdr.commands.filter((c) => c.argv.includes('prompt')).length;
      if (pre.screen.busy || pre.screen.dialog) stop('the session was not visibly idle before the wake trigger');
      const wakeTrig = fire('wake');
      const wakeFrame = await waitWire('the wake notification', (es) => channelFrames('wake-test')(es)[0], num('wireTimeoutMs'), { label: 'wake-wait' });
      // Before waiting for the wake turn to end, wait (bounded) for it to visibly begin --
      // the screen shows work or differs from the pre-wake read -- so the next operator
      // prompt cannot race a turn that has not started drawing yet. Not seeing it is
      // recorded, not fatal: a fast turn can start and end between two reads.
      let wakeTurnSeen = null;
      const wakeStartDeadline = deadlineFor(num('wireTimeoutMs'));
      while (!wakeTurnSeen && Date.now() < wakeStartDeadline) {
        const r = await read('wake-turn-start-poll', { keep: 'on-change' });
        if (r.screen.dialog) await handleDialog(r, 'wake-turn');
        else if (r.screen.busy || normalizeDialogText(r.text) !== normalizeDialogText(pre.text)) wakeTurnSeen = { seq: r.seq, busy: r.screen.busy };
        else await sleep(pollMs);
      }
      const afterWake = await settle('wake-turn', num('turnTimeoutMs'));
      const wakeRead = await read('after-wake', { source: 'recent-unwrapped', lines: readLines });
      g1.wake = {
        preReadSeq: pre.seq,
        preReadBusy: pre.screen.busy,
        preReadDialog: pre.screen.dialog,
        operatorPromptsBeforeWake: promptsBefore,
        trigger: wakeTrig,
        wire: { line: wakeFrame.line, t: wakeFrame.t, id: wakeFrame.id, metaKeys: wakeFrame.metaKeys, droppedKeys: wakeFrame.droppedKeys },
        turnStartSeen: wakeTurnSeen,
        settledReadSeq: afterWake.seq,
        afterReadSeq: wakeRead.seq,
      };

      // --- 4. attribute query and the dropped meta key -------------------------------------
      const aq = await prompt('attributePrompt', prompts.attributePrompt);
      await settle('attribute-query', num('turnTimeoutMs'));
      const aqRead = await read('after-attribute-query', { source: 'recent-unwrapped', lines: readLines });
      g1.attributeQuery = { prompt: aq, answerReadSeq: aqRead.seq };

      // --- 5. busy turn, two mid-turn notifications --------------------------------------
      const bp = await prompt('busyPrompt', prompts.busyPrompt);
      const busyDeadline = deadlineFor(num('turnTimeoutMs'));
      let firstBusy = null;
      while (!firstBusy) {
        if (Date.now() >= busyDeadline) stop('the busy turn never visibly started (no in-progress indicator on screen)');
        const r = await read('busy-start-poll', { keep: true });
        if (r.screen.dialog) await handleDialog(r, 'busy-turn');
        else if (r.screen.busy) firstBusy = r;
        else await sleep(pollMs);
      }
      // Keep reading while the turn runs, so the pane record brackets each notification.
      const pollUntil = async (until, label) => {
        while (Date.now() < until) {
          const r = await read(label, { keep: true });
          if (r.screen.dialog) await handleDialog(r, 'busy-turn');
          await sleep(Math.min(pollMs, Math.max(0, until - Date.now())));
        }
      };
      await pollUntil(Date.now() + num('midturnDelayMs'), 'busy-poll');
      const t1 = fire('midturn');
      const n1 = await waitWire('mid-turn notification 1', (es) => channelFrames('midturn-test')(es)[0], num('wireTimeoutMs'), { keep: true, label: 'midturn-1-wait' });
      const r1 = await read('after-midturn-1');
      await pollUntil(Date.parse(t1.at) + num('midturnGapMs'), 'busy-poll');
      const t2 = fire('midturn');
      const n2 = await waitWire('mid-turn notification 2', (es) => channelFrames('midturn-test')(es)[1], num('wireTimeoutMs'), { keep: true, label: 'midturn-2-wait' });
      const r2 = await read('after-midturn-2');
      await settle('busy-turn-end', num('turnTimeoutMs'));
      const busyRead = await read('after-busy-turn', { source: 'recent-unwrapped', lines: readLines });
      g1.busyTurn = {
        prompt: bp,
        firstBusySeq: firstBusy.seq,
        notifications: [
          { trigger: t1, wire: { line: n1.line, t: n1.t, id: n1.id }, readAfterSeq: r1.seq },
          { trigger: t2, wire: { line: n2.line, t: n2.t, id: n2.id }, readAfterSeq: r2.seq },
        ],
        afterReadSeq: busyRead.seq,
      };

      // --- 6. reply tool call -----------------------------------------------------------
      const rp = await prompt('replyPrompt', prompts.replyPrompt);
      const call = await waitWire(
        'the reply tool call and its result',
        (es) => transcriptFacts(es).replyCalls.find((c) => c.resultLine !== null) ?? null,
        num('turnTimeoutMs'),
        { label: 'reply-wait' },
      );
      await settle('reply-turn', num('turnTimeoutMs'));
      const replyRead = await read('after-reply', { source: 'recent-unwrapped', lines: readLines });
      g1.reply = { prompt: rp, wire: call, afterReadSeq: replyRead.seq };

      const post = await harnessVersions(['claude']);
      g1.postRunVersion = post.claude;
      if (parseClaudeCliVersion(post.claude) !== cli) ctx.finding(`claude --version changed during the run: ${cliRaw} before, ${post.claude} after`);
      await herdr.paneProcessInfo(ws.paneId);
    } finally {
      // Mid-turn windows, computed from what is on record (pane reads and wire times).
      if (g1.busyTurn) {
        const parsed = parseSections(sections.join(''));
        const input = herdr.commands.filter((c) => INPUT_ROLES.has(c.role)).map((c) => ({ seq: c.seq, role: c.role, startedAt: c.startedAt }));
        for (const n of g1.busyTurn.notifications) {
          n.midTurn = midTurnWindow({ notificationT: n.wire.t, sections: parsed, promptSubmittedAt: g1.busyTurn.prompt.endedAt, inputCommands: input, busyIndicator });
        }
      }
      ctx.record('g1', g1);
      if (g1.fixtures) {
        if (sections.length) ctx.capture(g1.fixtures.pane, sections.join(''));
        if (serverDir && existsSync(transcriptPath())) ctx.capture(g1.fixtures.transcript, readFileSync(transcriptPath(), 'utf8'), { format: 'jsonl' });
      }
    }
  },
};
