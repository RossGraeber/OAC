// g1-claude-wake: G1 re-run end to end through the herdr driver (Epic K, K4 #127).
//
// NOT VERDICT-BEARING. This scenario re-runs every probe of the human-run G1 Box C
// (docs/planning/gates/G1-result.md) through herdr and records the run so it can be compared
// against Box C, criterion by criterion (tools/herdr/lib/g1-report.mjs). It never changes
// G1's verdict, STATUS.md, or PINS.md.
//
// LIVE STATUS: RUN LIVE; NO CURRENT EQUIVALENCE RECORD.
// docs/planning/gates/herdr-runs/G1-2026-09-29.md: run 20260929T034856Z-05b135, herdr
// reporting `herdr 0.9.1` (executable identity UNVERIFIED: schemaVersion 1 manifest, #252)
// and Claude Code 2.1.283, run outcome PASS, accept=human, driver commit 3050ed6. Since #252
// (dated note, 2026-10-03) it stays on record but is not a current equivalence record: its
// manifest records no herdr executable or sha256.
// A G1 run under the current driver must re-establish equivalence; a later run relies on a
// record only under oac-gates references/scripted-runs.md "When a scripted run may carry a
// verdict" (among other conditions, an empty tools/herdr/ diff, test/ excluded, against the
// record's driver commit). That run's pane capture confirms the dev-channels dialog and the
// `esc to interrupt` indicator (the record's findings); other Claude Code pane-text patterns
// (lib/g1.mjs) are confirmed only as far as a committed record shows them.
//
// Operator command (a machine with herdr at the PINS.md pin and a logged-in Claude Code, any
// version: versions float, and one other than PINS.md's last tested version is a VERSION
// WARNING finding, never a stop (#216); the launch below is the default and may be omitted):
//
//   node tools/herdr/run.mjs --scenario g1-claude-wake \
//     --launch '["claude","--dangerously-load-development-channels","server:g1spike"]' \
//     --out <run dir>
//   node tools/herdr/lib/g1-report.mjs --run <run dir>            # draft comparison
//
// Accept policy (#196, docs/planning/decisions/K-196-driver-accepts-dialogs.md). The default is
// accept=driver: the driver accepts Claude Code's folder-trust ("No, exit" preselected: it
// moves down to "Yes, I trust this folder", verified by a read, then Enter), project-MCP-server
// ("Continue without…" preselected: up, up to "Use this MCP server") and dev-channels dialogs,
// and records each accept as `driver`. Any other dialog (tool permission, anything not on
// record) is refused: NOT RUN, no key. Operator decision on #196 (2026-09-30): G1 is fully
// driver-accepted by default; criterion 5 (the dev-channels consent step) is then `not
// evaluable`, so such a run is never a G1 equivalence record. For a run that is meant to be
// one, pass --param accept=human: the driver then sends no key and the operator accepts.
// Optional: --param projectDir=<absolute path outside the repository> reuses one directory, so
// a folder trust and MCP approval Claude Code recorded for it (after a human or driver accept)
// persist and those dialogs do not come back; the driver never writes that trust itself.
//
// What it does, in Box C's order:
//   0. Preflight. The launch must be G1's, verbatim. `claude --version` is compared with the
//      `Claude Code (Channels)` minimum and last tested versions in PINS.md AS COMMITTED at
//      HEAD (an uncommitted PINS.md edit is a finding). A difference is a VERSION WARNING
//      finding and the run continues: versions float, warn, never gate (#216). The run never
//      edits PINS.md. The quarantined channel server is staged into the run's
//      scratch directory from its blob committed at HEAD, and the copy's sha256 checked
//      against that blob and against the sha256 Box C ran; a working-tree file that differs
//      from HEAD (edited, replaced, symlinked) refuses the run. It is never edited or
//      imported. A project `.mcp.json` in
//      a scratch project directory registers the copy as `g1spike` (the way the G1 operator
//      registered it); no harness config is written.
//   1. Launch through `herdr agent start --kind claude -- <launch[1..]>`. Every dialog is
//      read from the pane verbatim BEFORE any keystroke reaches it. With --param
//      accept=human the driver sends nothing and waits for the operator to accept; with
//      accept=driver (default) it accepts only a dialog it recognizes whose options on screen are
//      exactly the ones on record (lib/g1.mjs DIALOG_KINDS), moving the selection one verified
//      key at a time before Enter (lib/gate-common.mjs driverAcceptDialog). The accept origin
//      is recorded. A driver-sent accept is never scored as meeting G1 criterion 5
//      (lib/g1-report.mjs).
//   2. Wait for the channel-server handshake on the wire. The wire clientInfo.version is
//      compared with PINS.md too (a VERSION WARNING, never a stop). Captures get the K4
//      fixture names only when the wire version equals the CLI version, so that a capture
//      names one version; otherwise they stay `unverified-*` (a finding; the run continues).
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

import { existsSync, lstatSync, readFileSync, utimesSync, writeFileSync } from 'node:fs';
import { dirname, isAbsolute, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { NotRunError, DriverError } from '../lib/herdr.mjs';
import { parseClaudeVersions, pinsReadWarning, parseClaudeCliVersion, claudeVersionWarning, CLAUDE_PIN_ROW } from '../lib/pins.mjs';
import { harnessVersions } from '../lib/manifest.mjs';
import { transcriptFacts, selectSegment, parseTranscript } from '../lib/compare-transcripts.mjs';
import { driverAcceptDialog, recordWaitState, takeBaseline, armActivityWatch, turnFloor, pastFloor, ACTIVITY_STATES } from '../lib/gate-common.mjs';
import {
  G1_LAUNCH, G1_SERVER_NAME, COMMITTED_SERVER, classifyScreen, driverMayAccept, mcpServerNames, dialogMatchesBoxC, DIALOG_KINDS,
  formatSection, parseSections, fixtureNames, unverifiedNames, stageServerCopy, committedFile, midTurnWindow, normalizeDialogText, sameDialog, acceptHint,
  COMMITTED_SERVER_SHA256, PINS_PATH,
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

// --param projectDir: an existing directory, reused across runs so that once Claude Code has
// recorded its folder trust (after an accept by a human, or by the driver under accept=driver,
// #196) the folder-trust dialog does not come up again. Optional friction reducer only. The
// driver never records that trust itself (Claude Code does, in its own state). It writes only the scenario's own `.mcp.json` there, and
// refuses a directory inside this repository or one whose `.mcp.json` registers anything
// but `g1spike`.
export function operatorProjectDir(dir) {
  const abs = resolve(String(dir));
  if (!isAbsolute(String(dir))) throw new DriverError('--param projectDir must be an absolute path');
  let st;
  try {
    st = lstatSync(abs);
  } catch {
    throw new DriverError(`--param projectDir ${abs} does not exist; create it and trust it in Claude Code first`);
  }
  if (st.isSymbolicLink() || !st.isDirectory()) throw new DriverError(`--param projectDir ${abs} is not a plain directory`);
  const rel = relative(REPO, abs);
  if (rel === '' || (!rel.startsWith('..') && !isAbsolute(rel))) throw new DriverError(`--param projectDir ${abs} is inside this repository; use a directory outside it`);
  const mcpPath = join(abs, '.mcp.json');
  if (existsSync(mcpPath)) {
    let names = null;
    try {
      names = Object.keys(JSON.parse(readFileSync(mcpPath, 'utf8')).mcpServers ?? {});
    } catch {
      /* unreadable: refused below */
    }
    if (!names || names.length !== 1 || names[0] !== G1_SERVER_NAME) {
      throw new DriverError(`${mcpPath} exists and is not this scenario's (it must register exactly "${G1_SERVER_NAME}"); refusing to overwrite it`);
    }
  }
  return abs;
}

export default {
  name: 'g1-claude-wake',
  description: 'G1 re-run through herdr for comparison with Box C (K4). Not verdict-bearing.',
  harnesses: ['claude'],
  defaults: {
    launch: [...G1_LAUNCH],
    timeboxMs: 45 * 60 * 1000, // Box C declared 45 minutes (G1-result.md "Timebox")
    params: {
      accept: 'driver', // operator decision on #196: G1 fully driver-accepted; criterion 5 then not evaluable; accept=human remains
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
      projectDir: '', // empty: a fresh scratch directory per run (folder-trust dialog every time)
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
      criterion5Rule: 'A driver-sent accept of the dev-channels dialog is never scored as meeting G1 criterion 5 (scripted-runs.md "Operator-consent dialogs"; kept by the #196 decisions, which make accept=driver the default and leave criterion 5 not evaluable on such runs).',
      params: { ...params, ...prompts },
      launch: { expected: [...G1_LAUNCH], actual: launch, verbatim: null },
      versions: null,
      server: null,
      projectDir: null,
      mcpJson: null,
      date: new Date().toISOString().slice(0, 10),
      fixtures: null, // K4 fixture names: set only after the CLI and wire versions are verified
      captureNames: null,
      agentStart: null,
      dialogs: [],
      herdrStates: [], // #253: every herdr agent wait, its agent_status and state_change_seq
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
    // Time spent waiting for the operator to accept a dialog. It has its own bound
    // (humanAcceptTimeoutMs), so an enclosing settle/wire wait does not also charge it
    // against its own budget (#154). The run's timebox still covers all of it.
    let humanWaitMs = 0;
    const leftUntil = (deadline, waitedAtStart) => Math.min(deadline + humanWaitMs - waitedAtStart, Date.now() + Math.max(0, ctx.remainingMs())) - Date.now();

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
        // Every key is sent straight after a read of AGENT; selection moves are verified by
        // reads; Enter only on the accepting option (lib/gate-common.mjs driverAcceptDialog).
        await driverAcceptDialog({ herdr, target: AGENT, r, d, kind, dialogKinds: DIALOG_KINDS, plan: driverMayAccept(r.screen, { expectedMcpServers: mcpServerNames(g1.mcpJson) }), read, stop, num, sleep, deadlineFor });
        return;
      }
      // Human accept: the driver sends nothing; it waits for the dialog to change.
      const before = herdr.commands.length;
      console.error(
        `\n[g1-claude-wake] dialog ${d.index} (${kind}) is on screen; its text is recorded (herdr command #${r.seq}).\n` +
          acceptHint({ sessionName: ctx.sessionName, agent: AGENT, kind, selected: r.screen.selected }) +
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
          // A redraw of the same dialog (resize on attach, scroll, moved selection) is not an
          // answer to it (#160): recorded, and the wait goes on.
          if (sameDialog(r.text, p, kind)) {
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

    // Wait until the pane shows neither a dialog nor work in progress. herdr agent state
    // only schedules the next read; the read decides. #253: a wait whose answer carries no
    // agent_status ends the run NOT RUN (recordWaitState), and `since` (a prompt typed with
    // `agent prompt --wait`, or an activity watch armed before a push) makes the settle wait
    // for that input's own turn: a settled state past the activity herdr observed
    // (lib/gate-common.mjs turnFloor). This wait never requests `unknown`.
    const settle = async (context, timeoutMs, { since = null } = {}) => {
      const deadline = deadlineFor(timeoutMs);
      const waited0 = humanWaitMs;
      const floor = await turnFloor({ since, g: g1, context, ctx, stop });
      if (!since) await sleep(num('settleMs'));
      let blockedUnseen = 0;
      for (;;) {
        const left = leftUntil(deadline, waited0);
        if (left <= 0) stop(`${context}: the pane did not settle within ${timeoutMs} ms`);
        const w = await herdr.agentWait(AGENT, { until: ['idle', 'done', 'blocked'], timeoutMs: Math.max(1000, left) });
        const st = recordWaitState({ g: g1, context, w, ctx, stop });
        const r = await read(`${context}-settled?`, { keep: 'on-change' });
        // herdr saying `blocked` while the screen shows no dialog the driver can name: herdr
        // state may lag the screen, so look again; if it persists, treat it as an
        // unrecognized dialog -- the conservative reading (never accepted by the driver).
        if (!r.screen.dialog && st.state === 'blocked') {
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
        if (r.screen.busy || !pastFloor(st, floor)) {
          await sleep(pollMs);
          continue;
        }
        r.settled = { state: st.state, waitSeq: st.seq, stateChangeSeq: st.stateChangeSeq, floor: floor.floor, turnBegunBy: floor.by };
        return r;
      }
    };

    // Wait for a frame on the wire; with watchPane, keep reading the pane (handling any
    // dialog, e.g. a tool permission prompt) every pollMs while waiting.
    const waitWire = async (what, pred, timeoutMs, { watchPane = true, keep = 'on-change', label = what } = {}) => {
      const deadline = deadlineFor(timeoutMs);
      const waited0 = humanWaitMs;
      let nextRead = 0;
      for (;;) {
        const hit = pred(lastInstance());
        if (hit) return hit;
        if (leftUntil(deadline, waited0) <= 0) stop(`timed out after ${timeoutMs} ms waiting for ${what} on the wire`);
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

    // #253: { wait: true } types the prompt only into an agent herdr reports idle/done, through
    // `herdr agent prompt --wait` (herdr observes the prompt's activity); the record is what
    // settle({ since }) takes. Without wait (the busy prompt): herdr's queued-state answer.
    const prompt = async (label, text, { wait = false } = {}) => {
      assertNotInjected(label, text);
      if (!wait) {
        const res = await herdr.agentPrompt(AGENT, text);
        return { seq: res.entry.seq, startedAt: res.entry.startedAt, endedAt: res.entry.endedAt, queued: { state: res.state, stateChangeSeq: res.stateChangeSeq } };
      }
      const base = await takeBaseline({ herdr, name: AGENT, g: g1, context: label, ctx, stop });
      if (ACTIVITY_STATES.includes(base.state)) stop(`herdr reported ${base.state} (herdr command #${base.seq}) just before the ${label}; a prompt is never typed into a running turn (#253); nothing sent`);
      const res = await herdr.agentPrompt(AGENT, text, { wait: { until: ['idle', 'done', 'blocked'], timeoutMs: num('turnTimeoutMs') } });
      const after = recordWaitState({ g: g1, context: `${label} --wait`, w: res, ctx, stop });
      return { kind: 'prompt-wait', seq: res.entry.seq, startedAt: res.entry.startedAt, endedAt: res.entry.endedAt, baseline: base, settledAtReturn: { state: after.state, stateChangeSeq: after.stateChangeSeq } };
    };

    try {
      // --- 0. preflight -------------------------------------------------------------------
      g1.launch.verbatim = JSON.stringify(launch) === JSON.stringify(G1_LAUNCH);
      if (!g1.launch.verbatim) {
        throw new DriverError(`launch ${JSON.stringify(launch)} is not G1's verbatim launch ${JSON.stringify(G1_LAUNCH)}; this would not be a G1 re-run`);
      }
      const cliRaw = ctx.harnessVersion('claude');
      // The version check reads PINS.md as committed at HEAD. Versions float and are never
      // gated (#216): a difference from the minimum or last tested version is a VERSION
      // WARNING finding, and an uncommitted PINS.md edit is a finding too. Neither stops the run.
      const pinsFile = committedFile(REPO, PINS_PATH);
      const pin = parseClaudeVersions(pinsFile.bytes.toString('utf8'));
      const cli = parseClaudeCliVersion(cliRaw);
      g1.versions = {
        pinsRow: CLAUDE_PIN_ROW,
        pinsMinimum: pin.minimum,
        pinsLastTested: pin.lastTested,
        pinsSource: { path: PINS_PATH, headCommit: pinsFile.headCommit, committedSha256: pinsFile.committedSha256, workingTreeMatchesHead: pinsFile.workingTreeMatchesHead },
        cliOutput: cliRaw,
        cli,
        wireClientInfo: null,
        verified: false, // true once the CLI and the wire report one and the same version
        matchesLastTested: null,
        warnings: [],
      };
      const warn = (w) => {
        if (!w) return;
        g1.versions.warnings.push(w);
        ctx.finding(w);
      };
      if (!pinsFile.workingTreeMatchesHead) ctx.finding(`${PINS_PATH} has uncommitted changes; the version check read the committed PINS.md (HEAD ${pinsFile.headCommit})`);
      warn(pinsReadWarning(pin, 'G1'));
      if (!cli && /^N\/A/.test(cliRaw ?? 'N/A')) stop(`claude --version could not be run (${cliRaw ?? 'not recorded'}); nothing launched`);
      warn(claudeVersionWarning({ observed: cli, lastTested: pin.lastTested, minimum: pin.minimum, source: '`claude --version`' }));
      // Captures get the K4 fixture names only once the CLI and the wire report the same
      // version (below), so a fixture names one version; until then they are `unverified-*`.
      g1.captureNames = unverifiedNames(g1.date);

      serverDir = ctx.dir('g1-server');
      // Staged from the blob committed at HEAD, never from whatever is on disk.
      const staged = stageServerCopy(REPO, COMMITTED_SERVER, serverDir);
      g1.server = {
        committed: COMMITTED_SERVER,
        headCommit: staged.headCommit,
        gitMode: staged.mode,
        committedSha256: staged.committedSha256,
        expectedSha256: COMMITTED_SERVER_SHA256,
        workingTreeSha256: staged.workingTreeSha256,
        workingTreeIsSymlink: staged.workingTreeIsSymlink,
        workingTreeMatchesHead: staged.workingTreeMatchesHead,
        copy: staged.copyPath,
        copySha256: staged.copySha256,
        match: staged.match,
      };
      if (staged.committedSha256 !== COMMITTED_SERVER_SHA256) throw new DriverError(`${COMMITTED_SERVER} at HEAD has sha256 ${staged.committedSha256}, not the ${COMMITTED_SERVER_SHA256} G1 Box C ran; refusing to run`);
      if (!staged.workingTreeMatchesHead) throw new DriverError(`${COMMITTED_SERVER} in the working tree differs from HEAD (edited, replaced or symlinked); refusing to run`);
      if (!staged.match) throw new DriverError('the staged channel-server copy does not match the committed blob (sha256)');

      const projectDir = params.projectDir ? operatorProjectDir(params.projectDir) : ctx.dir('g1-project');
      if (params.projectDir) ctx.redactLiteral(projectDir, '<PROJECT>');
      g1.projectDir = params.projectDir ? 'operator-supplied (--param projectDir), redacted as <PROJECT>' : 'fresh scratch directory';
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
      const wireV = /^\d+\.\d+\.\d+$/.test(String(g1.versions.wireClientInfo ?? '')) ? g1.versions.wireClientInfo : null;
      warn(claudeVersionWarning({ observed: wireV, lastTested: pin.lastTested, minimum: pin.minimum, source: 'the wire initialize clientInfo.version' }));
      if (cli && wireV === cli) {
        g1.versions.verified = true; // CLI and wire report one and the same version
        g1.versions.matchesLastTested = cli === pin.lastTested;
        g1.fixtures = fixtureNames(g1.date, cli);
        g1.captureNames = g1.fixtures;
      } else {
        ctx.finding(`the wire clientInfo.version (${g1.versions.wireClientInfo ?? 'none'}) differs from \`claude --version\` (${cliRaw}); the run continues, but its captures stay unverified-* because they cannot name one Claude Code version`);
      }
      await herdr.paneProcessInfo(ws.paneId);
      await settle('post-handshake', num('startupTimeoutMs'));
      if (!g1.devChannelsDialogSeen) ctx.finding('the dev-channels confirmation dialog was never recognized on screen before the session settled (G1 criterion 5)');

      // --- 3. idle wake -------------------------------------------------------------------
      const pre = await read('pre-wake-idle-check');
      const promptsBefore = herdr.commands.filter((c) => c.argv.includes('prompt')).length;
      if (pre.screen.busy || pre.screen.dialog) stop('the session was not visibly idle before the wake trigger');
      // #253: a baseline and an activity watch before the push; the wake-turn settle waits for it.
      const wakeBase = await takeBaseline({ herdr, name: AGENT, g: g1, context: 'wake-push', ctx, stop });
      const wakeWatch = await armActivityWatch({ herdr, name: AGENT, base: wakeBase, timeoutMs: num('turnTimeoutMs'), armMs: pollMs, sleep });
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
      const afterWake = await settle('wake-turn', num('turnTimeoutMs'), { since: wakeWatch });
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
      const aq = await prompt('attributePrompt', prompts.attributePrompt, { wait: true });
      await settle('attribute-query', num('turnTimeoutMs'), { since: aq });
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
      const rp = await prompt('replyPrompt', prompts.replyPrompt, { wait: true });
      const call = await waitWire(
        'the reply tool call and its result',
        (es) => transcriptFacts(es).replyCalls.find((c) => c.resultLine !== null) ?? null,
        num('turnTimeoutMs'),
        { label: 'reply-wait' },
      );
      await settle('reply-turn', num('turnTimeoutMs'), { since: rp });
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
      if (g1.captureNames) {
        if (sections.length) ctx.capture(g1.captureNames.pane, sections.join(''));
        if (serverDir && existsSync(transcriptPath())) ctx.capture(g1.captureNames.transcript, readFileSync(transcriptPath(), 'utf8'), { format: 'jsonl' });
      }
    }
  },
};
