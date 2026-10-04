// Shared pieces of the scripted G1 re-run (Epic K, K4 #127): the verbatim G1 launch, the
// committed quarantined channel server and how it is staged, the Box C baseline, pane-text
// classification used for SCHEDULING and SAFETY only, and the pane-capture section format.
//
// Nothing here scores a criterion on herdr agent state. Pane text is read verbatim and kept;
// the classification below only decides whether the driver may continue (a dialog is up, a
// turn is still visibly running) and never stands in for evidence.
//
// Every Claude Code pane-text pattern below except the dev-channels dialog is UNVERIFIED
// against a live Claude Code: only that dialog's text is on record (G1-result.md, Box C,
// criterion 5). The others are best guesses to be confirmed or corrected by the first
// operator run; an unrecognized dialog is never accepted by the driver.

import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { sha256, committedFile } from './committed-file.mjs';

// G1's launch, verbatim (G1-result.md "Original run" command transcript summary; Box C used
// the same command). launch[0] is the herdr agent kind; herdr runs `claude` and passes the
// rest through unchanged (K1 §5 item 2).
export const G1_LAUNCH = Object.freeze(['claude', '--dangerously-load-development-channels', 'server:g1spike']);
export const G1_SERVER_NAME = 'g1spike';

export const FIXTURE_DIR = 'docs/planning/gates/fixtures/g1-claude-wake';
export const COMMITTED_SERVER = `${FIXTURE_DIR}/channel-server.mjs.throwaway-quarantined`;
export const BOX_C_TRANSCRIPT = `${FIXTURE_DIR}/transcript-2026-09-28-2.1.283-boxC.jsonl`;
export const HERDR_RUNS_DIR = 'docs/planning/gates/herdr-runs';
export const PINS_PATH = 'docs/planning/PINS.md';

// sha256 of COMMITTED_SERVER as committed (G1 Box C ran this file unmodified); a guard that
// the committed blob itself has not been swapped since K4 was written.
export const COMMITTED_SERVER_SHA256 = 'b6a3f2b65a836a09a8d01ec70be1396f00968767792b7ce62217121b58adc48f';

// The five G1 pass criteria, verbatim from .claude/skills/oac-gates/references/G1-claude-wake.md.
export const G1_CRITERIA = Object.freeze([
  'Server declares `capabilities.experimental["claude/channel"] = {}` and negotiates a legacy MCP revision (`2025-11-25` or earlier); use `MCP_PROTOCOL_NEGOTIATION=legacy` for stdio servers if needed.',
  'A `notifications/claude/channel` carrying `content` (string) and `meta` (string-to-string map) wakes an idle session as a user turn and appears as a `<channel>` tag with the expected attributes (one attribute per identifier-safe `meta` key).',
  'A second notification sent mid-turn is queued and delivered at the next turn, in order (not dropped, not interleaved out of order).',
  'Claude replies through an ordinary MCP tool (conventionally `reply`).',
  'The `--dangerously-load-development-channels` interactive confirmation dialog is actually exercised during the spike — not bypassed, scripted around, or skipped.',
]);

// The dev-channels dialog as the Box C operator captured it before accepting
// (G1-result.md, criterion 5). The markdown escape `\>` there is the selection marker.
export const BOX_C_DIALOG_LINES = Object.freeze([
  'WARNING: Loading development channels',
  '--dangerously-load-development-channels is for local channel development only.',
  'Do not use this option to run channels you have downloaded off the internet.',
  'Please use --channels to run a list of approved channels.',
  'Channels: server:g1spike',
  '1. I am using this for local development',
  '2. Exit',
  'Enter to confirm · Esc to cancel',
]);

// Box C's <channel> attribute set after the wake (G1-result.md, criterion 2).
export const BOX_C_WAKE_ATTRIBUTES = Object.freeze({ source: 'g1spike', oac_message_id: 'g1-spike-wake-test-1', oac_sender: 'g1-spike-operator' });

// sha256 and committedFile live in committed-file.mjs (#139 review); re-exported here.
export { sha256, committedFile };

// Stage the quarantined server into the run's scratch directory as `channel-server.mjs`
// (Node needs the .mjs name to load it as a module). The copy is written from the blob
// COMMITTED at HEAD and its sha256 re-checked against that blob; the working-tree file's
// own state is reported alongside (the scenario refuses to run when it differs). The file
// is never edited and never imported.
export function stageServerCopy(repoRoot, relPath, destDir) {
  const c = committedFile(repoRoot, relPath);
  const copyPath = join(destDir, 'channel-server.mjs');
  writeFileSync(copyPath, c.bytes);
  const { bytes, ...rest } = c;
  return { ...rest, ...verifyServerCopy(copyPath, c.committedSha256) };
}

export function verifyServerCopy(copyPath, committedSha256) {
  const copySha = sha256(readFileSync(copyPath));
  return { copyPath, copySha256: copySha, match: committedSha256 === copySha };
}

// --- pane text: scheduling and safety only -----------------------------------------------

const BOX_CHARS = /[─-╿▀-▟]/g; // box drawing and block elements
const SELECT_MARK = /^[\s│|]*(?:[❯›>▶▸→*])\s*(\d+)\.\s*(.+?)\s*[│|]*\s*$/;
// An unnumbered option list (Claude Code v2.1.283's folder-trust dialog, #156:
// "❯ No, exit" / "  Yes, I trust this folder"). Only the arrow-like markers: `>` and `*`
// also start prompt and bullet lines, so they count only before a number.
const SELECT_MARK_UNNUMBERED = /^[\s│|]*[❯›▶▸→]\s*(\S.*?)\s*[│|]*\s*$/;

// Driver accepts (#196, docs/planning/decisions/K-196-driver-accepts-dialogs.md). A kind with
// `options` lists its option texts EXACTLY as seen live, in screen order; `preselected` is the
// index Claude Code selected on first read, `accept` the index the driver selects before Enter.
// The driver accepts such a dialog only when the pane shows exactly those options, in that
// order, with the selection on `preselected` or `accept`; it then moves the selection one key
// at a time, re-reading the pane after each key, and presses Enter only when a read shows
// `accept` selected (planDriverAccept here, driverAcceptDialog in gate-common.mjs). Anything
// else is refused: the run ends NOT RUN, and no keystroke is guessed. A kind without `options`
// (tool-permission) is never driver-accepted (#197 review): NOT RUN, no key.
// Optional per-kind fields (#199, Codex's trust dialog, lib/g2.mjs CODEX_DIALOG_KINDS):
//   numbered  the options are numbered on record: each must carry its number (1, 2, …) in
//             order, and body text above the first option is not an option whatever its
//             indentation (a numbered extra option is still refused);
//   marker    the one selection marker on record: a selection shown with any other marker is
//             refused;
//   footer    the kind's own footer pattern (default: DEFAULT_FOOTER); when set, a read
//             without it is refused, and a selection marker below it counts;
//   body      the question paragraph between the detect line and the options, verbatim: the
//             pane must show it whole or as a prefix ending in "…";
//   note      an optional block above the question: if shown, it must be exactly this text;
//   refuse    the driver never answers this kind, and this is why (#204: Codex's startup
//             hook review is the operator's consent decision). planDriverAccept refuses it
//             with this text; under accept=human the driver waits for the operator as usual.
// #267: the multi-select variant of 'mcp-server-approval', recorded from the live G4 herdr run
// of 2026-10-04 (run 20261004T040635Z-8ac610, Claude Code 2.1.285 on Windows; pane capture
// unverified-pane-claude-2026-10-04-herdr.txt, section seq 16), verbatim:
//
//     3 new MCP servers found in this project
//     Select any you wish to enable.
//
//     MCP servers may execute code or access system resources. All tool calls require approval. Learn more in the MCP
//     documentation.
//
//     ❯ [✔] g4spike
//       [✔] g4modern
//       [✔] g4http
//          Enable selected
//    Space to select · Esc to reject all
//
// The driver accepts it (#196 ruling on #267) only when ALL of these hold on the read it plans
// from (planMcpMultiSelect) and on every read that verifies a move (multiSelectCheck):
//   - the heading, intro, body and footer are exactly the recorded text, and the heading's
//     count equals the number of server rows;
//   - every other line between heading and footer is a server row `[✔] <name>` / `[ ] <name>`
//     or the one "Enable selected" row, last; nothing else, exactly one ❯ marker;
//   - the listed server names equal EXACTLY the scenario's expected servers (its .mcp.json):
//     an extra, missing or duplicate name is refused;
//   - every listed server is shown ticked (✔);
//   - the selection is on the preselection on record (the first server) or on "Enable
//     selected"; the driver moves it with `down` keys, each verified by a read, and sends
//     Enter only after a read shows "Enable selected" selected.
// Space is never sent: the driver never changes a tick. Anything else: refused, NOT RUN.
// The ❯ rendering on the "Enable selected" row is not on record (only the first-read
// preselection was seen); a verifying read that does not show exactly one ❯, on that row,
// never leads to Enter.
export const MCP_MULTISELECT = Object.freeze({
  heading: /^[ \t]*(\d+) new MCP servers? found in this project[ \t]*$/m,
  intro: 'Select any you wish to enable.',
  body: 'MCP servers may execute code or access system resources. All tool calls require approval. Learn more in the MCP documentation.',
  footer: /^[ \t]*Space to select · Esc to reject all[ \t]*$/m,
  tick: '✔',
  marker: '❯',
  submit: 'Enable selected',
  verified: 'herdr G4 run 2026-10-04 (20261004T040635Z-8ac610), Claude Code v2.1.285 on Windows; first server preselected, all three ticked (#267)',
});

export const DIALOG_KINDS = Object.freeze({
  'dev-channels': {
    // Box C, verbatim (the one live-observed dialog); the same text and preselection were seen
    // again in the 2026-09-30 L3 probe runs (Claude Code v2.1.283, Windows).
    detect: /Loading development channels/i,
    acceptOption: /^I am using this for local development\b/i,
    options: Object.freeze(['I am using this for local development', 'Exit']),
    preselected: 0,
    accept: 0,
    verified: 'G1-result.md Box C (criterion 5), Claude Code v2.1.283; again in the 2026-09-30 L3 probe runs',
  },
  'workspace-trust': {
    detect: /trust the files in this folder|Do you trust this folder|trust this (?:folder|project)|Is this a project you (?:created|trust)/i,
    acceptOption: /^Yes, I trust this folder$/i,
    // Detect pattern and option text seen live (#156, and the 2026-09-30 L3 probe run 3):
    // "Is this a project you created or one you trust?", options "❯ No, exit" (preselected) /
    // "  Yes, I trust this folder", unnumbered, footer "Enter to confirm · Esc to cancel".
    // The refusing option is preselected, so the driver moves down one before Enter.
    options: Object.freeze(['No, exit', 'Yes, I trust this folder']),
    preselected: 0,
    accept: 1,
    verified: 'herdr runs 2026-09-29 (#156) and 2026-09-30 (L3 probe run 3), Claude Code v2.1.283 on Windows; "No, exit" preselected',
  },
  'mcp-server-approval': {
    detect: /New MCP servers? found in (?:this project|\.mcp\.json)|MCP servers? (?:found|defined) in \.mcp\.json/i,
    // The driver approves this one server only, never "all future MCP servers".
    acceptOption: /^Use this MCP server$/i,
    // Seen live (#161, and the 2026-09-30 L3 probe runs): "New MCP server found in this
    // project: <name>", unnumbered options "Use this MCP server" / "Use this and all future MCP
    // servers in this project" / "Continue without using this MCP server" (the last preselected).
    options: Object.freeze(['Use this MCP server', 'Use this and all future MCP servers in this project', 'Continue without using this MCP server']),
    preselected: 2,
    accept: 0,
    verified: 'herdr runs 2026-09-29 (#161) and 2026-09-30 (L3 probe runs), Claude Code v2.1.283 on Windows; "Continue without using this MCP server" preselected',
    // #267: the same dialog kind in a newer, multi-select form (first seen on Claude Code
    // 2.1.285; harness versions float, #216, so nothing gates on the version). See
    // MCP_MULTISELECT above for its recorded text and the rules the driver applies to it.
    variants: Object.freeze({ 'multi-select': MCP_MULTISELECT }),
  },
  'tool-permission': {
    detect: /Do you want to (?:proceed|allow|make this edit)|Allow (?:this )?tool/i,
    acceptOption: /^Yes\b/i,
    verified: null, // UNVERIFIED: Box C's tool-permission prompts are not on record
    // No `options`: recognized (so it is recorded and scheduled around) but NEVER accepted by
    // the driver (#197 review; tool approval is outside #196).
  },
});

// A blocking prompt whose wording matches none of the kinds above.
const GENERIC_DIALOG = /Enter to confirm|Esc to cancel|Esc to exit|\(y\/n\)/i;

// -> { number, text } for a numbered list, { number: null, text } for an unnumbered one, or
// null. A numbered selection anywhere on screen wins over an unnumbered one.
export function selectedOption(text) {
  const lines = String(text).split(/\r?\n/).map((l) => l.replace(BOX_CHARS, ' '));
  for (const line of lines) {
    const m = SELECT_MARK.exec(line);
    if (m) return { number: Number(m[1]), text: m[2].replace(/\s{2,}\d+\.\s.*$/, '').trim() };
  }
  for (const line of lines) {
    const m = SELECT_MARK_UNNUMBERED.exec(line);
    if (m) return { number: null, text: m[1].trim() };
  }
  return null;
}

// The option lines of a kind's dialog, as the pane shows them: every line between the line
// the kind's detect pattern matches and the footer whose text (marker, numbering and box
// drawing removed) equals one of the kind's known option texts. -> [{ text, number, selected }]
// in screen order, plus `marked`, the count of selection-marked lines in that region (an
// option the driver does not know, marked as selected, makes marked exceed the matches).
// null for a kind without `options`.
const OPTION_LINE = /^\s*([❯›▶▸→>*])?\s*(?:(\d+)\.\s+)?(.*?)\s*$/;
// Claude Code's footers ("Enter to confirm · Esc to cancel"); a kind may name its own (`footer`).
const DEFAULT_FOOTER = /(?:Press )?Enter to (?:confirm|continue)[^\n]*|Esc to (?:cancel|exit)[^\n]*|\(y\/n\)[^\n]*/i;
export function dialogOptions(text, kind, dialogKinds = DIALOG_KINDS) {
  const def = dialogKinds[kind];
  if (!def?.options) return null;
  const s = String(text ?? '');
  const m = def.detect.exec(s);
  if (!m) return null;
  const rest = s.slice(s.lastIndexOf('\n', m.index) + 1);
  const foot = (def.footer ?? DEFAULT_FOOTER).exec(rest);
  // A kind that records its own footer must show exactly it (#201 review): a different footer
  // (Codex's "enter continue and create sandbox · esc back") or none at all is off record.
  if (def.footer && !foot) {
    const r = [];
    r.marked = 0;
    r.unknown = ['(recorded footer not on screen)'];
    return r;
  }
  // Numbered options run together on one line (Box C's transcription: "1. … 2. Exit") are
  // split at the next number, as selectedOption reads them.
  const region = (foot ? rest.slice(0, foot.index) : rest)
    .split(/\r?\n/)
    .map((l) => l.replace(BOX_CHARS, ' '))
    .flatMap((l) => l.split(/\s{2,}(?=\d+\.\s)/));
  const found = [];
  const unknown = [];
  let marked = 0;
  const parsed = region.map((line) => {
    const [, mark, num, body] = OPTION_LINE.exec(line);
    // `>` and `*` count as selection markers only before a number (they also start prompt and
    // bullet lines), as in selectedOption.
    const selected = !!mark && (!/[>*]/.test(mark) || num !== undefined);
    const col = line.length - line.replace(/^\s*(?:[❯›▶▸→>*]\s*)?/, '').length; // column of the option text
    return { selected, mark: selected ? mark : null, num, body, col };
  });
  // Unnumbered options line up with the selected line's text; a line at that column (or any
  // numbered line) that is not a known option is an option the driver does not know.
  // After the first option-shaped line (a known option, or a marked line), every non-empty line
  // up to the footer must be a known option, whatever its indentation (#197 review).
  // A numbered kind (#199) has body text at the options' column above them (Codex's wrapped
  // "Trust this folder? …" paragraph); there, only a numbered line before the options is one.
  const optionCol = def.numbered ? null : (parsed.find((p) => p.selected)?.col ?? null);
  let inOptions = false;
  const bodyLines = [];
  for (const p of parsed) {
    if (p.selected) marked += 1;
    if (!p.body) continue;
    const known = def.options.includes(p.body);
    if (known || p.selected) inOptions = true;
    if (known) found.push({ text: p.body, number: p.num === undefined ? null : Number(p.num), selected: p.selected, mark: p.mark });
    else if (inOptions || p.num !== undefined || (optionCol !== null && p.col === optionCol)) unknown.push(p.body);
    else bodyLines.push(p.body);
  }
  // A kind that records its question paragraph (`body`, #201 review): the text between the
  // detect line and the first option must be that paragraph, whole, or a prefix of it ending
  // in "…" (Codex truncates it to fit). Anything else there is off record.
  if (def.body && !bodyMatches(bodyLines, def.body)) unknown.push('(question text off record)');
  // A kind that records an optional block above its question (`note`): if the block is on
  // screen, it must be exactly the recorded text.
  if (def.note) {
    const above = s.slice(0, s.lastIndexOf('\n', m.index) + 1).split(/\r?\n/);
    const at = above.findIndex((l) => /^\s*Note:/.test(l));
    if (at !== -1) {
      // The block may wrap; its lines are read until they add up to the recorded text's
      // length (the repository-root path follows it directly and is not checked).
      let got = '';
      for (const l of above.slice(at)) {
        if (!l.trim() || got.length >= def.note.length) break;
        got = `${got} ${l.trim()}`.replace(/\s+/g, ' ').trim();
      }
      if (got !== def.note) unknown.push('(note text off record)');
    }
  }
  // Below a recorded footer: another selection-marked line (a second dialog, a stray picker)
  // counts as a second marker (#201 review).
  if (def.footer && foot) {
    for (const line of rest.slice(foot.index + foot[0].length).split(/\r?\n/)) {
      const [, mark, num] = OPTION_LINE.exec(line.replace(BOX_CHARS, ' '));
      if (mark && (!/[>*]/.test(mark) || num !== undefined)) marked += 1;
    }
  }
  found.marked = marked;
  found.unknown = unknown;
  return found;
}

function bodyMatches(lines, recorded) {
  let t = lines.join(' ').replace(/\s+/g, ' ').trim();
  if (t === recorded) return true;
  if (!t.endsWith('…')) return false;
  t = t.slice(0, -1).trimEnd();
  return t.length > 0 && recorded.startsWith(t);
}

// --- #267: the multi-select MCP approval form ----------------------------------------------

// A server row: optional ❯, "[<tick>] <name>", nothing else. "Enable selected": its own row.
const MS_ROW = /^\s*(❯)?\s*\[(.)\] (\S+)\s*$/u;
const MS_SUBMIT = (v) => new RegExp(`^\\s*(❯)?\\s*${v.submit.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}\\s*$`);
const MS_ANY_MARK = /^\s*[❯›▶▸→]/;

// The multi-select form as the pane shows it, or null when its recorded heading is not on
// screen. -> { count, servers: [{ name, tick, ticked, selected }], submit: { selected } | null,
// marked, footer, unknown } where `unknown` lists every line between heading and footer that
// is not the recorded text or a row of the recorded shape (and '(…)' notes for missing parts).
export function mcpMultiSelectForm(text, v = MCP_MULTISELECT) {
  const s = String(text ?? '').replace(/\r/g, '');
  const h = v.heading.exec(s);
  if (!h) return null;
  const rest = s.slice(h.index + h[0].length);
  const foot = v.footer.exec(rest);
  const region = (foot ? rest.slice(0, foot.index) : rest).split('\n').map((l) => l.replace(BOX_CHARS, ' ')).filter((l) => l.trim());
  const submitRe = MS_SUBMIT(v);
  const form = { count: Number(h[1]), servers: [], submit: null, marked: 0, footer: !!foot, unknown: [] };
  if (!foot) form.unknown.push('(recorded footer not on screen)');
  let i = 0;
  if (region[i]?.trim() === v.intro) i += 1;
  else form.unknown.push('(intro text off record)');
  const bodyLines = [];
  while (i < region.length && !MS_ROW.test(region[i]) && !submitRe.test(region[i]) && !MS_ANY_MARK.test(region[i])) bodyLines.push(region[i++].trim());
  if (bodyLines.join(' ').replace(/\s+/g, ' ') !== v.body) form.unknown.push('(body text off record)');
  for (; i < region.length; i += 1) {
    const line = region[i];
    if (MS_ANY_MARK.test(line)) form.marked += 1;
    const r = MS_ROW.exec(line);
    const sub = submitRe.exec(line);
    if (r && !form.submit) form.servers.push({ name: r[3], tick: r[2], ticked: r[2] === v.tick, selected: !!r[1] });
    else if (sub && !form.submit) form.submit = { selected: !!sub[1] };
    else form.unknown.push(line.trim());
  }
  if (!form.submit) form.unknown.push(`(no "${v.submit}" row)`);
  // A selection marker below the footer (a second dialog, a stray picker) counts too.
  if (foot) for (const line of rest.slice(foot.index + foot[0].length).split('\n')) if (MS_ANY_MARK.test(line.replace(BOX_CHARS, ' '))) form.marked += 1;
  return form;
}

// The row the selection is on: a server name, the submit row's text, or null.
const msCurrent = (f, v) => (f.submit?.selected ? v.submit : (f.servers.find((x) => x.selected)?.name ?? null));

// How may the DRIVER accept the multi-select form? Same result shape as planDriverAccept, plus
// variant, listedServers, expectedServers (recorded on the dialog), `from` (the row selected
// on the first read) and `verify` (what each verifying read must still show).
export function planMcpMultiSelect(classification, expectedServers, v = MCP_MULTISELECT) {
  const f = classification?.form ?? null;
  const listed = f ? f.servers.map((x) => x.name) : [];
  const expected = Array.isArray(expectedServers) ? [...expectedServers] : null;
  const base = { variant: 'multi-select', listedServers: listed, expectedServers: expected };
  const no = (why) => ({ ok: false, why: `the mcp-server-approval multi-select form: ${why}; the driver does not guess keystrokes`, moves: [], keys: [], ...base });
  if (!f) return no('not read');
  if (!expected?.length) return no('this scenario named no expected MCP servers, and the driver enables no server it was not told to expect');
  if (f.unknown.length) return no(`text off record on screen (${JSON.stringify(f.unknown)})`);
  if (f.count !== listed.length) return no(`the heading counts ${f.count} servers but ${listed.length} are listed`);
  const dup = listed.filter((n, i) => listed.indexOf(n) !== i);
  if (dup.length) return no(`server(s) listed twice: ${JSON.stringify([...new Set(dup)])}`);
  const extra = listed.filter((n) => !expected.includes(n));
  const missing = expected.filter((n) => !listed.includes(n));
  if (extra.length || missing.length) return no(`the listed servers ${JSON.stringify(listed)} are not exactly the expected ${JSON.stringify(expected)} (extra: ${JSON.stringify(extra)}, missing: ${JSON.stringify(missing)})`);
  const unticked = f.servers.filter((x) => !x.ticked);
  if (unticked.length) return no(`server(s) not shown ticked: ${JSON.stringify(unticked.map((x) => `[${x.tick}] ${x.name}`))}; the driver never changes a tick`);
  if (f.marked !== 1) return no(`${f.marked} selection markers on screen, not exactly one`);
  const from = msCurrent(f, v);
  let moves;
  if (from === v.submit) moves = [];
  else if (from === listed[0]) moves = [...listed.slice(1), v.submit].map((expect) => ({ key: 'down', expect }));
  else return no(`the selected row (${JSON.stringify(from)}) is not "${v.submit}" nor the preselection on record (the first server, ${JSON.stringify(listed[0])})`);
  return { ok: true, why: null, moves, keys: [...moves.map((m) => m.key), 'enter'], from, verify: { variant: 'multi-select', servers: listed }, ...base };
}

// selectionCheck for the multi-select form: the same server rows in the same order, every one
// still ticked, exactly one marker, and that marker on `expect`.
function multiSelectCheck(screen, expect, prev, verify, v = MCP_MULTISELECT) {
  const f = screen?.variant === 'multi-select' ? screen.form : null;
  if (!f) return { state: 'wait', why: 'no multi-select form read' };
  if (f.unknown.length) return { state: 'wait', why: `text off record on screen (${JSON.stringify(f.unknown)})` };
  const names = f.servers.map((x) => x.name);
  if (JSON.stringify(names) !== JSON.stringify(verify.servers)) return { state: 'stop', why: `the listed servers are now ${JSON.stringify(names)}, not ${JSON.stringify(verify.servers)}` };
  if (f.servers.some((x) => !x.ticked)) return { state: 'stop', why: `a server is no longer shown ticked (${JSON.stringify(f.servers.filter((x) => !x.ticked).map((x) => x.name))})` };
  if (f.marked !== 1) return { state: 'wait', why: `${f.marked} selection markers on screen, not exactly one` };
  const now = msCurrent(f, v);
  if (now === expect) return { state: 'ok' };
  if (now === prev) return { state: 'wait', why: `the selection is still ${JSON.stringify(prev)}` };
  return { state: 'stop', why: `the selection is ${JSON.stringify(now)}, not ${JSON.stringify(expect)}` };
}

// -> { dialog: kind | 'unknown' | null, selected, options, busy }
//   busy: the pane shows Claude Code's in-progress indicator (default "esc to interrupt";
//   UNVERIFIED wording, a scenario parameter). options: dialogOptions() for a kind that lists
//   its options, else null.
export function classifyScreen(text, { busyIndicator = 'esc to interrupt' } = {}) {
  const s = String(text ?? '');
  let dialog = null;
  for (const [kind, def] of Object.entries(DIALOG_KINDS)) {
    if (def.detect.test(s)) {
      dialog = kind;
      break;
    }
  }
  if (!dialog && GENERIC_DIALOG.test(s)) dialog = 'unknown';
  const busy = busyIndicator ? s.toLowerCase().includes(busyIndicator.toLowerCase()) : false;
  // #267: a kind's recorded variant (the multi-select MCP form) is read with its own parser.
  for (const [variant, v] of Object.entries((dialog && DIALOG_KINDS[dialog]?.variants) || {})) {
    const form = mcpMultiSelectForm(s, v);
    if (form) return { dialog, variant, form, selected: selectedOption(s), options: null, busy };
  }
  return { dialog, selected: dialog ? selectedOption(s) : null, options: dialog ? dialogOptions(s, dialog) : null, busy };
}

const optLabel = (o) => (o ? `"${o.number == null ? '' : `${o.number}. `}${o.text}"` : 'none found');

// The kind-specific form checks on an option list whose texts already match the record (#199):
// numbering (a `numbered` kind: option i carries number i + 1) and the selection marker (a kind
// with `marker`: the selected option shows exactly that marker). -> null, or why not.
function formOffRecord(opts, def) {
  if (def.numbered) {
    const nums = opts.map((o) => o.number);
    if (nums.some((n, i) => n !== i + 1)) return `the options are numbered ${JSON.stringify(nums)}, not ${JSON.stringify(opts.map((_, i) => i + 1))} as on record`;
  }
  if (def.marker) {
    const bad = opts.find((o) => o.selected && o.mark !== def.marker);
    if (bad) return `the selection marker is ${JSON.stringify(bad.mark)}, not the ${JSON.stringify(def.marker)} on record`;
  }
  return null;
}

// How may the DRIVER accept this dialog? -> { ok, why, moves: [{ key, expect }], keys }
//   moves: the selection keys to send, one at a time, each followed by a read that must show
//          `expect` selected; keys: every key in order, Enter last.
// Only a recognized kind WITH `options` on record (see DIALOG_KINDS): the pane must show
// exactly its known options, in order, one of them selected, and the selection must be on the
// preselection on record or already on the accepting option. A kind without `options` is
// refused. Never a guessed keystroke.
export function planDriverAccept(classification, dialogKinds = DIALOG_KINDS, { expectedMcpServers = null } = {}) {
  const kind = classification?.dialog;
  const def = dialogKinds[kind];
  const no = (why) => ({ ok: false, why, moves: [], keys: [] });
  if (!def) return no(`unrecognized dialog (${kind ?? 'none'}); the driver never accepts a dialog it cannot name`);
  if (def.refuse) return no(def.refuse);
  // #267: a recorded variant (the multi-select MCP form) has its own rules: planMcpMultiSelect.
  if (classification.variant) {
    const v = def.variants?.[classification.variant];
    if (!v) return no(`${kind}: variant ${classification.variant} is not on record`);
    return planMcpMultiSelect(classification, expectedMcpServers, v);
  }
  // #197 review: a kind with no option text on record (Claude Code's tool-permission prompt)
  // is never driver-accepted, whatever is preselected. The driver accepts only the dialogs
  // K-196 lists (three Claude Code ones and, since #199, Codex's workspace trust); anything
  // else ends the run NOT RUN with no key sent.
  if (!def.options) return no(`${kind}: no option text on record; the driver accepts only the dialogs listed in K-196 (use accept=human)`);
  const sel = classification.selected;
  if (!sel) return no('no selected option found in the dialog text');
  const opts = classification.options ?? [];
  const texts = opts.map((o) => o.text);
  if (opts.unknown?.length || JSON.stringify(texts) !== JSON.stringify(def.options)) {
    if (opts.unknown?.length) texts.push(...opts.unknown.map((u) => `?${u}`));
    return no(`the ${kind} options on screen (${JSON.stringify(texts)}) are not the ones on record (${JSON.stringify(def.options)}); the driver does not guess keystrokes`);
  }
  const off = formOffRecord(opts, def);
  if (off) return no(`the ${kind} dialog: ${off}; the driver does not guess keystrokes`);
  const selectedIdx = opts.map((o, i) => (o.selected ? i : -1)).filter((i) => i !== -1);
  if (selectedIdx.length !== 1 || opts.marked !== 1) return no(`the ${kind} dialog does not show exactly one selected option on record (${opts.marked} marked)`);
  const at = selectedIdx[0];
  if (at !== def.accept && at !== def.preselected) {
    return no(`the selected option (${optLabel(opts[at])}) is not the ${kind} accepting option, nor the preselection on record ("${def.options[def.preselected]}")`);
  }
  const step = def.accept > at ? 1 : -1;
  const moves = [];
  for (let i = at; i !== def.accept; i += step) moves.push({ key: step > 0 ? 'down' : 'up', expect: def.options[i + step] });
  return { ok: true, why: null, moves, keys: [...moves.map((mv) => mv.key), 'enter'] };
}

// Does a read taken after a selection key show the move landed cleanly (#197 review)? The
// read must show the kind's options exactly as on record, exactly ONE selection marker, and
// that marker on `expect`. -> { state: 'ok' } | { state: 'wait', why } (not yet, or a read
// that is not clean: the driver reads again until its bound, and never sends Enter on it) |
// { state: 'stop', why } (the selection moved somewhere else).
export function selectionCheck(screen, kind, expect, prev, dialogKinds = DIALOG_KINDS, verify = null) {
  const def = dialogKinds[kind];
  // #267: a move planned on a recorded variant is verified by that variant's rules.
  if (verify?.variant) {
    const v = def?.variants?.[verify.variant];
    return v ? multiSelectCheck(screen, expect, prev, verify, v) : { state: 'stop', why: `variant ${verify.variant} is not on record` };
  }
  if (screen?.variant) return { state: 'wait', why: `the read shows the ${screen.variant} form, not the one planned from` };
  const opts = screen?.options;
  if (!def?.options || !opts) return { state: 'wait', why: 'no option list read' };
  if (opts.unknown?.length || JSON.stringify(opts.map((o) => o.text)) !== JSON.stringify(def.options)) return { state: 'wait', why: `options on screen ${JSON.stringify(opts.map((o) => o.text))} are not the ones on record` };
  if (opts.marked !== 1) return { state: 'wait', why: `${opts.marked} selection markers on screen, not exactly one` };
  const off = formOffRecord(opts, def);
  if (off) return { state: 'wait', why: off };
  const now = opts.find((o) => o.selected)?.text ?? null;
  if (now === expect) return { state: 'ok' };
  if (now === prev) return { state: 'wait', why: `the selection is still ${JSON.stringify(prev)}` };
  return { state: 'stop', why: `the selection is ${JSON.stringify(now)}, not ${JSON.stringify(expect)}` };
}

// May the DRIVER accept this dialog (possibly after moving the selection)? The plan's ok/why.
// expectedMcpServers: the scenario's .mcp.json server names, which the multi-select MCP form
// must list exactly (#267); without them that form is refused.
export function driverMayAccept(classification, { expectedMcpServers = null } = {}) {
  return planDriverAccept(classification, DIALOG_KINDS, { expectedMcpServers });
}

// A driverMayAccept bound to a scenario's expected MCP servers, read when a dialog is planned
// (a scenario writes its .mcp.json after creating its agents). getNames() -> string[] | null.
export const driverMayAcceptExpecting = (getNames) => (classification) => driverMayAccept(classification, { expectedMcpServers: getNames() ?? null });
// The server names of a scenario's recorded .mcp.json ({ path, contents }), or null.
export const mcpServerNames = (mcpJson) => (mcpJson?.contents?.mcpServers ? Object.keys(mcpJson.contents.mcpServers) : null);

// Whitespace-, box-drawing- and selection-marker-insensitive comparison of captured dialog
// text against Box C's. Lines of Box C's text that cannot be found are listed.
export function normalizeDialogText(s) {
  return String(s)
    .replace(BOX_CHARS, ' ')
    .replace(/[❯›▶▸→]/g, ' ')
    .replace(/(^|\s)>(?=\s)/g, ' ')
    .replace(/\s+/g, ' ')
    .trim();
}

// The dialog's own text: from the start of the line its kind's detect pattern matches (or,
// for an unrecognized dialog, the whole screen) to the end of its footer ("Enter to confirm
// …"), with box drawing, selection markers and ALL whitespace removed. A re-wrap (an attach
// resizes the pane), a scroll of lines above the dialog, or a moved selection leaves it
// unchanged; a different dialog does not (#160).
export function dialogBody(text, kind, dialogKinds = DIALOG_KINDS) {
  let s = String(text ?? '');
  // #267: a recorded variant on screen brings its own footer.
  const variant = Object.values(dialogKinds[kind]?.variants ?? {}).find((v) => v.heading.test(s.replace(/\r/g, '')));
  if (variant) s = s.replace(/\r/g, '');
  const m = dialogKinds[kind]?.detect.exec(s);
  const start = m ? s.lastIndexOf('\n', m.index) + 1 : 0;
  const rest = s.slice(start);
  const foot = (variant?.footer ?? dialogKinds[kind]?.footer ?? DEFAULT_FOOTER).exec(rest);
  const body = foot ? rest.slice(0, foot.index + foot[0].length) : rest;
  return normalizeDialogText(body).replace(/\s+/g, '');
}

// Is `after` (a classified read) still the dialog first read as `beforeText`? Only then is a
// changed screen NOT the dialog being answered (#160).
export function sameDialog(beforeText, after, kind, dialogKinds = DIALOG_KINDS) {
  return after.screen.dialog === kind && dialogBody(after.text, kind, dialogKinds) === dialogBody(beforeText, kind, dialogKinds);
}

// What the operator is told while the driver waits for a human accept (#162). "Press Enter"
// only when the preselected option is the kind's accepting one; otherwise the operator is
// told what is preselected and to move the selection first.
export function acceptHint({ sessionName, agent, kind, selected, dialogKinds = DIALOG_KINDS }) {
  const def = dialogKinds[kind];
  const sel = selected ? `"${selected.number == null ? '' : `${selected.number}. `}${selected.text}"` : 'none found';
  const attach = `herdr session attach ${sessionName}`;
  if (def && selected && def.acceptOption.test(selected.text)) {
    return `  Preselected: ${sel}, the accepting option. Accept it yourself: attach with \`${attach}\` and press Enter, or run \`herdr --session ${sessionName} agent send-keys ${agent} enter\`.\n`;
  }
  const why = def ? `, which is NOT this dialog's accepting option` : ' (dialog not recognized)';
  return `  Preselected: ${sel}${why}. Do not just press Enter: attach with \`${attach}\`, read the dialog, move the selection to the option you mean, then press Enter.\n`;
}

export function dialogMatchesBoxC(text) {
  const hay = normalizeDialogText(text);
  const missing = BOX_C_DIALOG_LINES.filter((l) => !hay.includes(normalizeDialogText(l)));
  return { matches: missing.length === 0, missing };
}

// --- pane capture sections --------------------------------------------------------------
//
// The pane fixture is one text file of sections, each a verbatim `agent read` with the
// recorded herdr command that produced it (seq and timestamps from the run manifest), so the
// fixture itself carries what mid-turn timing is established from.

const SECTION_OPEN = '### oac-herdr-pane-section ';
const SECTION_CLOSE = '### oac-herdr-pane-section-end ';

export function formatSection({ seq, label, source, startedAt, endedAt }, text) {
  const head = JSON.stringify({ seq, label, source, startedAt, endedAt });
  const body = String(text ?? '').replace(/\n$/, '');
  return `${SECTION_OPEN}${head}\n${body}\n${SECTION_CLOSE}${JSON.stringify({ seq })}\n`;
}

export function parseSections(fileText) {
  const out = [];
  const lines = String(fileText).split('\n');
  for (let i = 0; i < lines.length; i++) {
    if (!lines[i].startsWith(SECTION_OPEN)) continue;
    const meta = JSON.parse(lines[i].slice(SECTION_OPEN.length));
    const body = [];
    let j = i + 1;
    for (; j < lines.length && !lines[j].startsWith(SECTION_CLOSE); j++) body.push(lines[j]);
    if (j >= lines.length) throw new Error(`pane section seq=${meta.seq} (${meta.label}) has no end marker`);
    out.push({ ...meta, text: body.join('\n') });
    i = j;
  }
  return out;
}

// --- mid-turn timing from wire timestamps against pane reads --------------------------
//
// A notification counts as sent mid-turn only when the record shows the busy turn visibly
// in progress on both sides of its wire timestamp:
//   - the busy prompt was submitted (its herdr command ended) before the notification's t;
//   - the last visible-screen read that ENDED before t, and started after the prompt, shows
//     the turn in progress (the in-progress indicator, or a tool-permission dialog, which
//     only appears inside a turn);
//   - the first visible-screen read that STARTED after t shows it still in progress;
//   - no read between the prompt and t showed the turn finished (idle) after it had
//     visibly started;
//   - no operator input other than the busy prompt was sent before that later read
//     (dialog accepts do not start a turn and are allowed).
// herdr agent state is not an input. The pane reads and the wire share one machine clock
// (the channel server runs locally). The unobserved window around t is reported, not
// assumed away.
export function midTurnWindow({ notificationT, sections, promptSubmittedAt, inputCommands = [], busyIndicator = 'esc to interrupt' }) {
  const reasons = [];
  const t = Date.parse(notificationT);
  const p = Date.parse(promptSubmittedAt);
  const out = { established: false, lastBeforeSeq: null, firstAfterSeq: null, gapBeforeMs: null, gapAfterMs: null, uncoveredWindowMs: null, reasons };
  if (!Number.isFinite(t)) return reasons.push('notification has no wire timestamp'), out;
  if (!Number.isFinite(p)) return reasons.push('busy prompt submission time not recorded'), out;
  if (!(t > p)) reasons.push('notification was sent before the busy prompt was submitted');
  const active = (s) => {
    const c = classifyScreen(s.text, { busyIndicator });
    return c.busy || c.dialog === 'tool-permission';
  };
  const visible = [...sections].filter((s) => s.source === 'visible').sort((a, b) => a.seq - b.seq);
  const inTurnBefore = visible.filter((s) => Date.parse(s.startedAt) >= p && Date.parse(s.endedAt) < t);
  const firstActive = inTurnBefore.findIndex(active);
  const lastBefore = inTurnBefore.at(-1);
  const firstAfter = visible.find((s) => Date.parse(s.startedAt) > t);
  if (!lastBefore) reasons.push('no pane read between the busy prompt and the notification');
  else if (!active(lastBefore)) reasons.push(`the last pane read before the notification (#${lastBefore.seq}) does not show the turn in progress`);
  if (firstActive !== -1 && inTurnBefore.slice(firstActive).some((s) => !active(s))) reasons.push('a pane read between the turn starting and the notification showed it not in progress');
  if (!firstAfter) reasons.push('no pane read after the notification');
  else if (!active(firstAfter)) reasons.push(`the first pane read after the notification (#${firstAfter.seq}) does not show the turn in progress`);
  const limit = firstAfter ? Date.parse(firstAfter.startedAt) : Infinity;
  const extra = inputCommands.filter((c) => c.role === 'operator-input' && Date.parse(c.startedAt) >= p && Date.parse(c.startedAt) < limit);
  if (extra.length) reasons.push(`operator input sent during the window (herdr command #${extra.map((c) => c.seq).join(', #')})`);
  if (lastBefore) {
    out.lastBeforeSeq = lastBefore.seq;
    out.gapBeforeMs = t - Date.parse(lastBefore.endedAt);
  }
  if (firstAfter) {
    out.firstAfterSeq = firstAfter.seq;
    out.gapAfterMs = Date.parse(firstAfter.startedAt) - t;
  }
  if (lastBefore && firstAfter) out.uncoveredWindowMs = Date.parse(firstAfter.startedAt) - Date.parse(lastBefore.endedAt);
  out.established = reasons.length === 0;
  return out;
}

// Fixture names K4 specifies: `<kind>-<YYYY-MM-DD>-<version>-herdr.<ext>`.
export function fixtureNames(date, version) {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(date)) throw new Error(`bad fixture date ${JSON.stringify(date)}`);
  if (!/^\d+\.\d+\.\d+$/.test(version)) throw new Error(`bad Claude Code version ${JSON.stringify(version)}`);
  return { transcript: `transcript-${date}-${version}-herdr.jsonl`, pane: `pane-${date}-${version}-herdr.txt` };
}

// Capture names for a run whose CLI and wire have not (yet) reported one and the same Claude
// Code version: deliberately not fixture-shaped, and never publishable.
export function unverifiedNames(date) {
  return { transcript: `unverified-transcript-${date}-herdr.jsonl`, pane: `unverified-pane-${date}-herdr.txt` };
}
