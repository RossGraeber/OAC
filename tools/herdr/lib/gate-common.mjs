// Pieces shared by the K8 (#131) scenarios g4-mcp-dual-era and g5-provenance: pinned
// criteria read from the committed gate reference, staging of the reconstructed gate servers,
// the two-pane agent helper (verbatim reads, dialogs read before any keystroke, settle), and
// a loopback port probe.
//
// The agent helper follows g1-claude-wake/g2-codex-inject exactly: pane text is read verbatim
// and kept with the herdr command that read it; herdr agent state is recorded and only
// schedules the next read; a dialog is on record before any keystroke reaches it, and the
// driver accepts only a dialog it can name, by the verified key plan in driverAcceptDialog
// (#196; a kind with no option list on record only when its accepting option is preselected).

import { lstatSync, readFileSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import { basename, join } from 'node:path';

import { committedFile, sha256, formatSection, normalizeDialogText, sameDialog, acceptHint, selectionCheck } from './g1.mjs';
import { NotRunError } from './herdr.mjs';

export class CriteriaDriftError extends Error {}

export const INPUT_ROLES = new Set(['operator-input', 'dialog-accept']);
export const GATE_SERVERS_DIR = 'tools/herdr/gate-servers';

// "## Pass criteria" checklist items of an oac-gates reference file, continuation lines joined.
export function parseCriteriaSection(referenceText, { count, path }) {
  const lines = String(referenceText).split(/\r?\n/);
  const start = lines.findIndex((l) => /^## Pass criteria/.test(l));
  if (start === -1) throw new Error(`${path} has no "## Pass criteria" section`);
  const items = [];
  for (const l of lines.slice(start + 1)) {
    if (/^## /.test(l)) break;
    const m = /^- \[[ x]\] (.*)$/.exec(l);
    if (m) items.push(m[1].trim());
    else if (items.length && /^\s+\S/.test(l)) items[items.length - 1] += ` ${l.trim()}`;
  }
  if (items.length !== count) throw new Error(`${path} lists ${items.length} pass criteria; expected ${count}`);
  return items;
}

// The criteria as COMMITTED at HEAD (never the working tree), bound to the sha256 of
// JSON.stringify(<parsed criteria>) the scoring was written against (K7's review pattern).
// Throws CriteriaDriftError on a mismatch, before anything is scored or written.
export function readPinnedCriteria(repoRoot, { path, count, pin, owner }) {
  const c = committedFile(repoRoot, path);
  const criteria = parseCriteriaSection(c.bytes.toString('utf8'), { count, path });
  const criteriaSha256 = sha256(JSON.stringify(criteria));
  const reference = { path, headCommit: c.headCommit, fileSha256: c.committedSha256, criteriaSha256, workingTreeMatchesHead: c.workingTreeMatchesHead };
  if (criteriaSha256 !== pin) {
    throw new CriteriaDriftError(
      `the pass criteria in ${path} at HEAD ${c.headCommit} hash to ${criteriaSha256}, not the ${pin} ${owner} scores against: ` +
        `the reference changed; re-review ${owner}'s scoring against the new criteria and move its pin in the same change. Nothing was scored or written`,
    );
  }
  return { criteria, reference };
}

// The same check on criteria handed to an evaluate function directly.
export function assertCriteriaPin(criteria, pin, owner) {
  const got = Array.isArray(criteria) ? sha256(JSON.stringify(criteria)) : null;
  if (got !== pin) throw new CriteriaDriftError(`the criteria given hash to ${got ?? 'nothing'}, not the ${pin} ${owner} is written against; the reference changed, re-review the scoring`);
}

// Stage reconstructed gate-server files into destDir (these are the driver's own committed
// tooling, not a quarantined spike blob). Each file's state is recorded: its sha256 as
// committed at HEAD (null when not committed), the raw working-tree sha256, whether the
// working tree matches HEAD (in git's normalized form, see committedFile), and the copy's
// sha256. When it matches, the COMMITTED bytes are staged, so a CRLF checkout stages the same
// bytes an LF one does (#152); otherwise the working-tree bytes are. A symlink is refused.
// Publishing a run (--write) requires every file to match HEAD and a clean tools/herdr/ (the
// report checks).
export function stageGateFiles(repoRoot, names, destDir) {
  return names.map((name) => {
    const rel = `${GATE_SERVERS_DIR}/${name}`;
    const abs = join(repoRoot, rel);
    if (lstatSync(abs).isSymbolicLink()) throw new Error(`${rel} is a symlink; refusing to stage it`);
    const workingBytes = readFileSync(abs);
    let c = null;
    try {
      c = committedFile(repoRoot, rel);
    } catch {
      /* not committed yet */
    }
    const workingTreeMatchesHead = c?.workingTreeMatchesHead === true;
    const bytes = workingTreeMatchesHead ? c.bytes : workingBytes;
    const copyPath = join(destDir, basename(name));
    writeFileSync(copyPath, bytes);
    const copySha256 = sha256(readFileSync(copyPath));
    return { path: rel, headCommit: c?.headCommit ?? null, committedSha256: c?.committedSha256 ?? null, workingTreeSha256: sha256(workingBytes), workingTreeMatchesHead, copy: copyPath, copySha256, match: copySha256 === sha256(bytes) };
  });
}

// Is 127.0.0.1:port free to listen on? Bounded; true, false, or null (could not tell).
export function loopbackPortFree(port, timeoutMs = 3000) {
  return new Promise((resolve) => {
    const s = createServer();
    const t = setTimeout(() => {
      s.close();
      resolve(null);
    }, timeoutMs);
    s.once('error', () => {
      clearTimeout(t);
      resolve(false);
    });
    s.listen(port, '127.0.0.1', () => {
      clearTimeout(t);
      s.close(() => resolve(true));
    });
  });
}

// A selection key is followed by reads until the pane shows the expected option selected;
// this bounds that wait. Nothing is ever re-sent when it expires.
export const DIALOG_MOVE_TIMEOUT_MS = 10000;

// The DRIVER accepts dialog `d` (first read as `r`) on agent `target` (#196). Every keystroke
// comes straight after a read of that target (herdr.dialogAccept refuses otherwise). The plan
// (planDriverAccept in g1.mjs) is made from the first read; each selection key is then
// verified by fresh reads that must still show the same dialog with the expected option
// selected, and Enter is sent only after a read shows the accepting option selected. After
// Enter the driver waits (bounded) for the screen to leave the dialog, so it is never answered
// twice (#160). Any deviation stops the run NOT RUN and nothing more is sent to the dialog.
// The record says what the driver sent: acceptOrigin 'driver', acceptKeys (each key with its
// herdr command seq and the read that verified it), acceptSeq (the Enter).
export async function driverAcceptDialog({ herdr, target, r, d, kind, dialogKinds, plan, read, stop, num, sleep, deadlineFor, label = '' }) {
  const who = `${label ? `${label} ` : ''}dialog ${d.index} (${kind})`;
  d.acceptPlan = plan.ok ? plan.keys : null;
  if (!plan.ok) {
    d.acceptOrigin = 'none (driver refused)';
    stop(`${who}: ${plan.why}; the driver did not accept it`);
  }
  d.acceptKeys = [];
  let last = r;
  let prev = r.screen.selected?.text ?? null;
  for (const [i, mv] of plan.moves.entries()) {
    const res = await herdr.dialogAccept(target, [mv.key]);
    const k = { key: mv.key, seq: res.entry.seq, expect: mv.expect, verifiedSeq: null };
    d.acceptKeys.push(k);
    const deadline = Date.now() + DIALOG_MOVE_TIMEOUT_MS;
    for (;;) {
      await sleep(Math.min(250, num('pollMs')));
      const p = await read(`dialog-${d.index}-select-${i + 1}`, { keep: 'on-change' });
      if (!sameDialog(r.text, p, kind, dialogKinds)) stop(`${who}: the screen left the dialog after selection key "${mv.key}" (herdr command #${res.entry.seq}), before Enter; nothing more sent`);
      // #197 review: exactly one marker, on the expected option, with the options on record.
      const c = selectionCheck(p.screen, kind, mv.expect, prev, dialogKinds);
      if (c.state === 'ok') {
        k.verifiedSeq = p.seq;
        last = p;
        break;
      }
      if (c.state === 'stop') stop(`${who}: after "${mv.key}" ${c.why}; nothing more sent`);
      if (Date.now() >= deadline) stop(`${who}: the selection did not move cleanly to ${JSON.stringify(mv.expect)} within ${DIALOG_MOVE_TIMEOUT_MS} ms of "${mv.key}" (last read: ${c.why}); Enter not sent, nothing re-sent`);
    }
    prev = mv.expect;
  }
  const res = await herdr.dialogAccept(target, ['enter']);
  d.acceptKeys.push({ key: 'enter', seq: res.entry.seq, expect: null, verifiedSeq: null });
  d.acceptOrigin = 'driver';
  d.acceptSeq = res.entry.seq;
  d.acceptAt = res.entry.startedAt;
  d.lastReadBeforeAcceptSeq = last.seq;
  d.selectionKeys = plan.moves.length;
  // Input other than this dialog's own selection keys between its first read and the Enter.
  const own = new Set(d.acceptKeys.map((x) => x.seq));
  d.inputBetweenReadAndAccept = herdr.commands.filter((c) => c.seq > r.seq && c.seq < res.entry.seq && INPUT_ROLES.has(c.role) && !own.has(c.seq)).length;
  const deadline = deadlineFor(num('humanAcceptTimeoutMs'));
  for (;;) {
    await sleep(num('pollMs'));
    const p = await read(`dialog-${d.index}-after-accept`, { keep: 'on-change' });
    if (!sameDialog(r.text, p, kind, dialogKinds)) {
      d.resolvedSeq = p.seq;
      return;
    }
    if (Date.now() >= deadline) stop(`${who} was still on screen ${num('humanAcceptTimeoutMs')} ms after the driver's accept; nothing re-sent`);
  }
}

// Two-pane agent helper. `g` is the scenario's record object (dialogs and herdrStates are
// appended to it, each tagged with the agent); `stop` ends the run NOT RUN.
export function makeAgent({ ctx, g, name, label, classify, dialogKinds, driverMayAccept, accept, num, stop }) {
  const { herdr } = ctx;
  const sections = [];
  const keptSeqs = new Set();
  let lastKeptKey = null;
  const sleep = (ms) => new Promise((r) => setTimeout(r, Math.max(0, ms)));
  const deadlineFor = (ms) => Date.now() + Math.min(ms, Math.max(0, ctx.remainingMs()));
  // Time spent waiting for the operator to accept a dialog: bounded by humanAcceptTimeoutMs
  // on its own, so an enclosing settle/waitFor does not also charge it to its budget (#154).
  let humanWaitMs = 0;
  const leftUntil = (deadline, waitedAtStart) => Math.min(deadline + humanWaitMs - waitedAtStart, Date.now() + Math.max(0, ctx.remainingMs())) - Date.now();

  const keep = (sec, text) => {
    if (keptSeqs.has(sec.seq)) return;
    sections.push(formatSection(sec, text));
    keptSeqs.add(sec.seq);
    lastKeptKey = `${sec.source}\n${text}`;
  };
  const read = async (lbl, { source = 'visible', lines, keep: k = true } = {}) => {
    const text = await herdr.agentRead(name, { source, lines, deadlineMs: 15000 });
    const e = herdr.commands.at(-1);
    const sec = { seq: e.seq, label: `${label}:${lbl}`, source, startedAt: e.startedAt, endedAt: e.endedAt };
    if (k === true || `${source}\n${text}` !== lastKeptKey) keep(sec, text);
    return { ...sec, text, screen: classify(text) };
  };

  const handleDialog = async (r, context) => {
    keep(r, r.text);
    if (g.dialogs.length >= num('maxDialogs')) stop(`more than ${num('maxDialogs')} dialogs; stopping`);
    const kind = r.screen.dialog;
    const d = { agent: label, index: g.dialogs.length + 1, kind, context, patternVerified: dialogKinds[kind]?.verified ?? null, readSeq: r.seq, readAt: r.startedAt, selected: r.screen.selected, acceptOrigin: null, acceptSeq: null, resolvedSeq: null, inputBetweenReadAndAccept: null };
    g.dialogs.push(d);
    if (accept === 'driver') {
      // driverMayAccept is the kind table's planner (planDriverAccept for Claude, Codex).
      await driverAcceptDialog({ herdr, target: name, r, d, kind, dialogKinds, plan: driverMayAccept(r.screen), read, stop, num, sleep, deadlineFor, label });
      return;
    }
    const before = herdr.commands.length;
    console.error(
      `\n[${name}] ${label} dialog ${d.index} (${kind}) is on screen; its text is recorded (herdr command #${r.seq}).\n` +
        acceptHint({ sessionName: ctx.sessionName, agent: name, kind, selected: r.screen.selected, dialogKinds }) +
        `  The driver sends no keystroke to it and waits up to ${num('humanAcceptTimeoutMs')} ms.\n`,
    );
    const waitStart = Date.now();
    const deadline = deadlineFor(num('humanAcceptTimeoutMs'));
    d.redrawSeqs = [];
    let shown = normalizeDialogText(r.text);
    try {
      for (;;) {
        if (Date.now() >= deadline) stop(`${label} dialog ${d.index} (${kind}) was not accepted by the operator within ${num('humanAcceptTimeoutMs')} ms`);
        await sleep(num('pollMs'));
        const p = await read(`dialog-${d.index}-waiting`, { keep: 'on-change' });
        // A redraw of the same dialog is not an answer to it (#160): recorded, wait goes on.
        if (sameDialog(r.text, p, kind, dialogKinds)) {
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

  // herdr's agent state, recorded, failing closed (#253): a wait whose response names no
  // state herdr documents ends the run NOT RUN, since nothing after it could be ordered on it.
  const unknownNoted = new Set();
  const waitState = async (context, timeoutMs) => {
    const w = await herdr.agentWait(name, { until: ['idle', 'done', 'blocked', 'unknown'], timeoutMs: Math.max(1000, timeoutMs) });
    const st = recordWaitState({ g, agent: label, context, w, ctx, stop });
    if (st.state === 'unknown' && !unknownNoted.has(context)) {
      unknownNoted.add(context);
      ctx.finding(`herdr reported agent state \`unknown\` for ${label} (${context}, herdr command #${w.entry.seq}); recorded, nothing re-sent (K1 §5 item 5)`);
    }
    return st;
  };

  // Wait until the pane shows neither a dialog nor work in progress, and herdr reports the
  // agent idle/done (#253). `since` is the record of a prompt herdr typed (prompt() below): the
  // settle then also requires that prompt's turn to have begun, by herdr's state_change_seq
  // moving past the prompt's (herdr's own --wait rule), by a busy screen seen in this settle,
  // or by `done()`, a wire-level check the scenario passes (Codex: thread/turns/list shows the
  // turn completed). Without one of those a wait that returned at once (herdr: "Standalone
  // `agent wait` returns immediately when the current status matches") only shows the state
  // from before the prompt was picked up. An `unknown` state is not settled unless `done()`
  // says the turn is over on the wire.
  const settle = async (context, timeoutMs, { since = null, done = null } = {}) => {
    if (since && since.stateChangeSeq == null && !done) stop(`${label} ${context}: herdr's prompt response (#${since.seq}) carried no state_change_seq, so the end of its turn cannot be told from the state before it (#253); nothing more sent`);
    await sleep(num('settleMs'));
    const deadline = deadlineFor(timeoutMs);
    const waited0 = humanWaitMs;
    let blockedUnseen = 0;
    let busySeen = false;
    let doneSeen = false;
    const wireDone = async () => (doneSeen ||= !!(done && (await done())));
    for (;;) {
      const left = leftUntil(deadline, waited0);
      if (left <= 0) stop(`${label} ${context}: the pane did not settle within ${timeoutMs} ms`);
      const st = await waitState(context, left);
      const r = await read(`${context}-settled?`, { keep: 'on-change' });
      if (r.screen.busy) busySeen = true;
      if (!r.screen.dialog && st.state === 'blocked') {
        if (++blockedUnseen < 3) {
          await sleep(num('pollMs'));
          continue;
        }
        r.screen = { ...r.screen, dialog: 'unknown', selected: null };
      } else blockedUnseen = 0;
      if (r.screen.dialog) {
        await handleDialog(r, context);
        continue;
      }
      if (r.screen.busy) {
        await sleep(num('pollMs'));
        continue;
      }
      const begun = promptTurnBegun({ since, stateChangeSeq: st.stateChangeSeq, busySeen }) || (await wireDone());
      if (!begun || (st.state === 'unknown' && !(await wireDone()))) {
        await sleep(num('pollMs'));
        continue;
      }
      r.settled = { state: st.state, waitSeq: st.seq, stateChangeSeq: st.stateChangeSeq, by: !since ? 'state' : busySeen ? 'busy screen seen' : doneSeen ? 'wire' : 'state_change_seq' };
      return r;
    }
  };

  // A full read (recent / recent-unwrapped with --lines) after a turn: settle first (#246), and
  // if herdr still refuses it with agent_not_idle (the agent went busy again between the settle
  // and the read), settle again and retry, a bounded number of times. Only reads are retried:
  // nothing is ever re-sent. If herdr keeps reporting `unknown` while the wire says the turn is
  // over, the read falls back to the visible screen, as herdr's docs advise, with a finding.
  const settledRead = async (lbl, { source = 'recent-unwrapped', lines } = {}, { context = lbl, timeoutMs, since = null, done = null, maxRefusals = 3 } = {}) => {
    for (let refusals = 0; ; ) {
      const s = await settle(context, timeoutMs, { since, done });
      const res = await herdr.agentReadResult(name, { source, lines, deadlineMs: 15000, allowErrorCodes: ['agent_not_idle'] });
      const e = res.entry;
      if (res.errorCode !== 'agent_not_idle') {
        const sec = { seq: e.seq, label: `${label}:${lbl}`, source, startedAt: e.startedAt, endedAt: e.endedAt };
        keep(sec, res.text);
        return { ...sec, text: res.text, screen: classify(res.text), settledSeq: s.seq };
      }
      g.notIdleRefusals = [...(g.notIdleRefusals ?? []), { agent: label, context, seq: e.seq, afterSettleSeq: s.seq, settledState: s.settled?.state ?? null }];
      if (s.settled?.state === 'unknown') {
        ctx.finding(`herdr refused the ${source} read for ${label} (${context}, herdr command #${e.seq}, agent_not_idle) while it reported \`unknown\` and the wire showed the turn over; the read fell back to the visible screen (herdr's documented alternative), so this capture holds less history`);
        const v = await read(lbl, { source: 'visible' });
        return { ...v, settledSeq: s.seq, fellBackToVisible: true };
      }
      if (++refusals >= maxRefusals) stop(`${label} ${context}: herdr refused the ${source} read ${refusals} times (agent_not_idle, last #${e.seq}) after the pane settled; nothing more sent`);
    }
  };

  // Poll a predicate while reading this pane every pollMs (kept on change), handling dialogs.
  const waitFor = async (what, pred, timeoutMs, { lbl = what, bail = null } = {}) => {
    const deadline = deadlineFor(timeoutMs);
    const waited0 = humanWaitMs;
    let nextRead = 0;
    for (;;) {
      if (herdr.abortSignal?.aborted) throw new NotRunError(herdr.abortReason());
      const hit = await pred();
      if (hit) return hit;
      const b = bail?.();
      if (b) return { bailed: b };
      if (leftUntil(deadline, waited0) <= 0) stop(`timed out after ${timeoutMs} ms waiting for ${what}`);
      if (Date.now() >= nextRead) {
        const r = await read(lbl, { keep: 'on-change' });
        if (r.screen.dialog) await handleDialog(r, what);
        nextRead = Date.now() + num('pollMs');
      }
      await sleep(200);
    }
  };

  // stateChangeSeq: the baseline settle({ since }) compares against (#253).
  const prompt = async (text) => {
    const res = await herdr.agentPrompt(name, text);
    return { seq: res.entry.seq, startedAt: res.entry.startedAt, endedAt: res.entry.endedAt, text, stateChangeSeq: res.stateChangeSeq };
  };

  return { name, label, sections, read, keep, handleDialog, waitState, settle, settledRead, waitFor, prompt };
}

// Record one `herdr agent wait` and fail closed on it (#253). A response whose agent_status
// herdr does not document (lib/herdr.mjs agentStatusOf: null) cannot establish the agent's
// state: it is a finding and the run ends NOT RUN, never "settled". Shared by makeAgent and
// the g1/g2 scenarios' own settle loops.
export function recordWaitState({ g, agent = null, context, w, ctx, stop }) {
  const rec = { ...(agent ? { agent } : {}), context, seq: w.entry.seq, state: w.state ?? null, stateChangeSeq: w.stateChangeSeq ?? null, durationMs: w.entry.durationMs ?? null };
  g.herdrStates.push(rec);
  if (rec.state === null) {
    ctx.finding(`herdr \`agent wait\` (herdr command #${rec.seq}${agent ? `, ${agent}` : ''}, ${context}) answered with no agent_status herdr documents, so the driver could not establish the agent's state; the run stops rather than proceed on it (#253)`);
    stop(`${agent ? `${agent} ` : ''}${context}: herdr agent wait #${rec.seq} did not report the agent's state; nothing more sent`);
  }
  return rec;
}

// Whether the turn a prompt started has begun, from what one settle saw (#253): herdr's
// state_change_seq moved past the prompt's (herdr's own `agent prompt --wait` rule,
// src/api/wait.rs at v0.9.1), or the screen showed work in progress, or the wire said so.
export function promptTurnBegun({ since, stateChangeSeq, busySeen = false, wireDone = false }) {
  if (!since) return true;
  return busySeen || wireDone || (stateChangeSeq != null && since.stateChangeSeq != null && stateChangeSeq > since.stateChangeSeq);
}

export const stopper = (herdr, g) => (reason) => {
  herdr.inputHalted ??= reason;
  g.stoppedAt = reason;
  throw new NotRunError(reason);
};
