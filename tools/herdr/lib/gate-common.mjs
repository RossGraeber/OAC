// Pieces shared by the K8 (#131) scenarios g4-mcp-dual-era and g5-provenance: pinned
// criteria read from the committed gate reference, staging of the reconstructed gate servers,
// the two-pane agent helper (verbatim reads, dialogs read before any keystroke, settle), and
// a loopback port probe.
//
// The agent helper follows g1-claude-wake/g2-codex-inject exactly: pane text is read verbatim
// and kept with the herdr command that read it; herdr agent state is recorded and only
// schedules the next read; a dialog is on record before any keystroke reaches it, and the
// driver accepts only a dialog it can name whose own preselected option is the accepting one.

import { lstatSync, readFileSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import { basename, join } from 'node:path';

import { committedFile, sha256, formatSection, normalizeDialogText } from './g1.mjs';
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

// Stage reconstructed gate-server files into destDir, from the WORKING TREE (these are the
// driver's own committed tooling, not a quarantined spike blob). Each file's state is
// recorded: its sha256 as committed at HEAD (null when not committed), the working-tree
// sha256, whether they match, and the copy's sha256. A symlink is refused. Publishing a run
// (--write) requires every file to match HEAD and a clean tools/herdr/ (the report checks).
export function stageGateFiles(repoRoot, names, destDir) {
  return names.map((name) => {
    const rel = `${GATE_SERVERS_DIR}/${name}`;
    const abs = join(repoRoot, rel);
    if (lstatSync(abs).isSymbolicLink()) throw new Error(`${rel} is a symlink; refusing to stage it`);
    const bytes = readFileSync(abs);
    let committedSha256 = null;
    let headCommit = null;
    try {
      const c = committedFile(repoRoot, rel);
      committedSha256 = c.committedSha256;
      headCommit = c.headCommit;
    } catch {
      /* not committed yet */
    }
    const copyPath = join(destDir, basename(name));
    writeFileSync(copyPath, bytes);
    const workingTreeSha256 = sha256(bytes);
    const copySha256 = sha256(readFileSync(copyPath));
    return { path: rel, headCommit, committedSha256, workingTreeSha256, workingTreeMatchesHead: committedSha256 === workingTreeSha256, copy: copyPath, copySha256, match: copySha256 === workingTreeSha256 };
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

// Two-pane agent helper. `g` is the scenario's record object (dialogs and herdrStates are
// appended to it, each tagged with the agent); `stop` ends the run NOT RUN.
export function makeAgent({ ctx, g, name, label, classify, dialogKinds, driverMayAccept, accept, num, stop }) {
  const { herdr } = ctx;
  const sections = [];
  const keptSeqs = new Set();
  let lastKeptKey = null;
  const sleep = (ms) => new Promise((r) => setTimeout(r, Math.max(0, ms)));
  const deadlineFor = (ms) => Date.now() + Math.min(ms, Math.max(0, ctx.remainingMs()));

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
      const may = driverMayAccept(r.screen);
      if (!may.ok) {
        d.acceptOrigin = 'none (driver refused)';
        stop(`${label} dialog ${d.index} (${kind}): ${may.why}; the driver did not accept it`);
      }
      const res = await herdr.dialogAccept(name, ['enter']);
      d.acceptOrigin = 'driver';
      d.acceptSeq = res.entry.seq;
      d.inputBetweenReadAndAccept = herdr.commands.filter((c) => c.seq > r.seq && c.seq < res.entry.seq && INPUT_ROLES.has(c.role)).length;
      // Never a second keystroke into the same dialog: wait (bounded) for the screen to leave
      // it before anything else reads it as a new one.
      const was = normalizeDialogText(r.text);
      const deadline = deadlineFor(num('humanAcceptTimeoutMs'));
      for (;;) {
        await sleep(num('pollMs'));
        const p = await read(`dialog-${d.index}-after-accept`, { keep: 'on-change' });
        if (p.screen.dialog !== kind || normalizeDialogText(p.text) !== was) {
          d.resolvedSeq = p.seq;
          return;
        }
        if (Date.now() >= deadline) stop(`${label} dialog ${d.index} (${kind}) was still on screen ${num('humanAcceptTimeoutMs')} ms after the driver's accept; nothing re-sent`);
      }
    }
    const before = herdr.commands.length;
    console.error(
      `\n[${name}] ${label} dialog ${d.index} (${kind}) is on screen; its text is recorded (herdr command #${r.seq}).\n` +
        `  Accept it yourself, e.g. \`herdr --session ${ctx.sessionName} agent send-keys ${name} enter\`, or attach and press Enter.\n` +
        `  The driver sends no keystroke to it and waits up to ${num('humanAcceptTimeoutMs')} ms.\n`,
    );
    const was = normalizeDialogText(r.text);
    const deadline = deadlineFor(num('humanAcceptTimeoutMs'));
    for (;;) {
      if (Date.now() >= deadline) stop(`${label} dialog ${d.index} (${kind}) was not accepted by the operator within ${num('humanAcceptTimeoutMs')} ms`);
      await sleep(num('pollMs'));
      const p = await read(`dialog-${d.index}-waiting`, { keep: 'on-change' });
      if (p.screen.dialog !== kind || normalizeDialogText(p.text) !== was) {
        d.acceptOrigin = 'human';
        d.resolvedSeq = p.seq;
        d.inputBetweenReadAndAccept = herdr.commands.slice(before).filter((c) => INPUT_ROLES.has(c.role)).length;
        return;
      }
    }
  };

  const waitState = async (context, timeoutMs) => {
    const w = await herdr.agentWait(name, { until: ['idle', 'done', 'blocked', 'unknown'], timeoutMs: Math.max(1000, timeoutMs) });
    const state = w.json?.result?.agent?.state ?? null;
    g.herdrStates.push({ agent: label, context, seq: w.entry.seq, state });
    if (state === 'unknown') ctx.finding(`herdr reported agent state \`unknown\` for ${label} (${context}, herdr command #${w.entry.seq}); recorded, nothing re-sent (K1 §5 item 5)`);
    return state;
  };

  const settle = async (context, timeoutMs) => {
    await sleep(num('settleMs'));
    const deadline = deadlineFor(timeoutMs);
    let blockedUnseen = 0;
    for (;;) {
      const left = deadline - Date.now();
      if (left <= 0) stop(`${label} ${context}: the pane did not settle within ${timeoutMs} ms`);
      const state = await waitState(context, left);
      const r = await read(`${context}-settled?`, { keep: 'on-change' });
      if (!r.screen.dialog && state === 'blocked') {
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
      return r;
    }
  };

  // Poll a predicate while reading this pane every pollMs (kept on change), handling dialogs.
  const waitFor = async (what, pred, timeoutMs, { lbl = what, bail = null } = {}) => {
    const deadline = deadlineFor(timeoutMs);
    let nextRead = 0;
    for (;;) {
      if (herdr.abortSignal?.aborted) throw new NotRunError(herdr.abortReason());
      const hit = await pred();
      if (hit) return hit;
      const b = bail?.();
      if (b) return { bailed: b };
      if (Date.now() >= deadline) stop(`timed out after ${timeoutMs} ms waiting for ${what}`);
      if (Date.now() >= nextRead) {
        const r = await read(lbl, { keep: 'on-change' });
        if (r.screen.dialog) await handleDialog(r, what);
        nextRead = Date.now() + num('pollMs');
      }
      await sleep(200);
    }
  };

  const prompt = async (text) => {
    const res = await herdr.agentPrompt(name, text);
    return { seq: res.entry.seq, startedAt: res.entry.startedAt, endedAt: res.entry.endedAt, text };
  };

  return { name, label, sections, read, keep, handleDialog, waitState, settle, waitFor, prompt };
}

export const stopper = (herdr, g) => (reason) => {
  herdr.inputHalted ??= reason;
  g.stoppedAt = reason;
  throw new NotRunError(reason);
};
