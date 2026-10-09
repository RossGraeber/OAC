// Shared pieces of the scripted G2 re-run (Epic K, K7 #130): the plain Codex launch, the
// committed quarantined G2 client and how it is staged, the human-run 0.157.1 baseline, the
// facts read from a G2 client transcript, the fixture sanitizer, the pane-process argv
// proof, and pane-text classification used for SCHEDULING and SAFETY only.
//
// Nothing here scores a criterion on herdr agent state. Turn completion and delivery come
// from the app-server event stream the quarantined client itself records; pane text is
// read verbatim and kept; the classification below only decides whether the driver may
// continue (a dialog is up) and never stands in for evidence.
//
// One Codex dialog is accepted by the driver: the workspace-trust dialog seen live on 0.159.2
// (#199), under the #197 rules. A second, Codex's MCP tool-approval prompt (seen live on 0.160.0,
// #271), is answered "1. Allow" only for the G4 scenario's own server and tools
// (planCodexToolApproval); every other scenario refuses it. A third, Codex's start-up update
// prompt (seen live on 0.160.0, #303), is answered "2. Skip" only, in every scenario, and an
// off-record form of it ends the run NOT RUN at once (planCodexUpdateSkip). Two more Codex startup screens are on record (#204, seen live
// on 0.159.2) and are NEVER answered by the driver: the startup hook review and the hooks
// browser it opens. The in-progress indicator ("esc to interrupt") was also seen live in the
// #204 runs. Every other Codex pane-text pattern below is UNVERIFIED against a live Codex
// TUI, a best guess; no other Codex dialog, recognized or not, is ever answered by the driver.
//
// Credential hygiene: nothing here opens anything under the Codex home directory. The only
// process data read is a pid's argv, from the run's process-table snapshot (lib/proc.mjs),
// never a process environment; only a minimized projection of it is recorded (#232, paneArgv).

import { readFileSync, writeFileSync } from 'node:fs';
import { basename, join } from 'node:path';

import { committedFile, sha256, selectedOption, normalizeDialogText, planDriverAccept, dialogOptions } from './g1.mjs';
import { diffSequences } from './compare-transcripts.mjs';
import { splitCommandLine, splitWindowsCommandLine } from './proc.mjs';

export { splitCommandLine, splitWindowsCommandLine };

// G2's launch: plain `codex`, no arguments and no config overrides (G2 criterion 1;
// G2-result.md: "the human operator ran plain `codex` (no `-c`, no `--remote`, no flags)").
// launch[0] is the herdr agent kind; herdr runs `codex` and passes nothing after it.
export const G2_LAUNCH = Object.freeze(['codex']);

export const FIXTURE_DIR = 'docs/planning/gates/fixtures/g2-codex-inject';
export const COMMITTED_CLIENT = `${FIXTURE_DIR}/client.mjs.throwaway-quarantined`;
export const BASELINE_TRANSCRIPT = `${FIXTURE_DIR}/transcript-2026-09-26-0.157.1.jsonl`;
export const HERDR_RUNS_DIR = 'docs/planning/gates/herdr-runs';
export const PINS_PATH = 'docs/planning/PINS.md';
export const MANIFEST_PATH = 'docs/planning/gates/fixtures/MANIFEST.json';

// sha256 of COMMITTED_CLIENT as committed (the G2 runs ran this file); a guard that the
// committed blob itself has not been swapped since K7 was written.
export const COMMITTED_CLIENT_SHA256 = '134d41a57e861a92f5381dce045e990cb77b7171aed8d5d81901ea6e6c4347d9';

// The four G2 pass criteria are read verbatim, at run time, from the gate's reference file
// as committed at HEAD, rather than restated here: criterion 4's wording names the provider
// and its credential store, which the containment lint (oac-boundaries check 10) keeps out
// of tools/herdr/. G2_CRITERIA_SHA256 below binds the text read to the text scored.
export const G2_REFERENCE = '.claude/skills/oac-gates/references/G2-codex-inject.md';
export function parseG2Criteria(referenceText) {
  const lines = String(referenceText).split(/\r?\n/);
  const start = lines.findIndex((l) => /^## Pass criteria/.test(l));
  if (start === -1) throw new Error(`${G2_REFERENCE} has no "## Pass criteria" section`);
  const items = [];
  for (const l of lines.slice(start + 1)) {
    if (/^## /.test(l)) break;
    const m = /^- \[[ x]\] (.*)$/.exec(l);
    if (m) items.push(m[1].trim());
    else if (items.length && /^\s+\S/.test(l)) items[items.length - 1] += ` ${l.trim()}`;
  }
  if (items.length !== 4) throw new Error(`${G2_REFERENCE} lists ${items.length} pass criteria; G2 has four`);
  return items;
}
// The scoring in lib/g2-report.mjs is written against exactly these four criteria, in this
// order. Their text is bound by this sha256 of JSON.stringify(<the four parsed criteria>),
// taken from the reference as committed when K7 was written (the hash carries none of the
// criterion text). A reworded, reordered, added or removed criterion changes it, and the
// report refuses to score until K7's scoring has been re-reviewed against the new text and
// this pin moved in the same change.
export const G2_CRITERIA_SHA256 = '012480fff690f067f608c16183949bf0eb9072e4d356f3024732418af105cd67';

export class CriteriaDriftError extends Error {}

// The criteria as COMMITTED at HEAD (never the working tree; K4's pattern), verified
// against G2_CRITERIA_SHA256. -> { criteria, reference: { path, headCommit, fileSha256,
// criteriaSha256, workingTreeMatchesHead } }. Throws CriteriaDriftError on a mismatch.
// `pin` exists only so the self-test can plant a drift in a throwaway repository.
export function readG2Criteria(repoRoot, { pin = G2_CRITERIA_SHA256 } = {}) {
  const c = committedFile(repoRoot, G2_REFERENCE);
  const criteria = parseG2Criteria(c.bytes.toString('utf8'));
  const criteriaSha256 = sha256(JSON.stringify(criteria));
  const reference = { path: G2_REFERENCE, headCommit: c.headCommit, fileSha256: c.committedSha256, criteriaSha256, workingTreeMatchesHead: c.workingTreeMatchesHead };
  if (criteriaSha256 !== pin) {
    throw new CriteriaDriftError(
      `the G2 pass criteria in ${G2_REFERENCE} at HEAD ${c.headCommit} hash to ${criteriaSha256}, not the ${pin} lib/g2-report.mjs scores against: ` +
        "the reference changed; re-review K7's scoring against the new criteria and move G2_CRITERIA_SHA256 (tools/herdr/lib/g2.mjs) in the same change. Nothing was scored or written",
    );
  }
  return { criteria, reference };
}

// Operator's own TUI message in the human 0.157.1 re-run (G2-result.md, thread/list preview
// "Greet me in a single word."); the default operator prompt here, typed as the operator
// typed it. It is the operator's input to their own TUI, never a delivered message.
export const DEFAULT_OPERATOR_PROMPT = 'Greet me in a single word.';

// Injection 1 of the human 0.157.1 re-run, word for word, with the observed version in place
// of 0.157.1 (fixture transcript-2026-09-26-0.157.1.jsonl line 16); the expected reply is the
// human run's. Until 2026-10-05 it also said "through herdr": in both live G2 runs
// (20261005T020547Z-84b913, 20261005T041011Z-bb584c) the model took those words as a task
// and used tools instead of replying with the acknowledgement, so the text now matches the
// baseline it is compared with. The busy turn and the queued message are the committed client's own
// `busyqueue` texts, unchanged.
export const defaultInjectText = (version) =>
  `G2 re-run on ${version} (from a second daemon client, not typed in this TUI): reply with exactly the words OAC G2 RERUN RECEIVED.`;
export const EXPECTED_REPLIES = Object.freeze({ inject: 'OAC G2 RERUN RECEIVED', queued: 'OAC G2 QUEUED' });
export const BUSY_TEXT_MARK = 'G2 spike busy-turn test';
export const QUEUED_TEXT_MARK = 'G2 spike queued message';

// Operator prompts and parameters may never carry a delivered message: delivery comes only
// from the second app-server client.
export function assertNotInjected(label, text) {
  const s = String(text);
  if (/OAC G2|second daemon client|thread\/queue\/add|turn\/start|G2 spike (?:busy-turn|queued)/i.test(s)) {
    throw new Error(`${label} looks like a delivered message; delivery into the thread comes only from the second app-server client, never typed by herdr`);
  }
}

// --- staging the committed client ---------------------------------------------------------

// Stage the quarantined client into destDir as `client.mjs` (Node needs the .mjs name), from
// the blob COMMITTED at HEAD, and re-check the copy's sha256. The working-tree file's own
// state is reported alongside (the scenario refuses to run when it differs). The file is
// never edited and never imported; it runs as its own process. It appends its wire
// transcript to `transcript.jsonl` beside itself, i.e. in destDir.
export function stageClientCopy(repoRoot, relPath, destDir) {
  const c = committedFile(repoRoot, relPath);
  const copyPath = join(destDir, 'client.mjs');
  writeFileSync(copyPath, c.bytes);
  const { bytes, ...rest } = c;
  const copySha256 = sha256(readFileSync(copyPath));
  return { ...rest, copyPath, copySha256, match: copySha256 === c.committedSha256 };
}

// --- fixture names ----------------------------------------------------------------------

export function fixtureNames(date, version) {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(date)) throw new Error(`bad fixture date ${JSON.stringify(date)}`);
  if (!/^\d+\.\d+\.\d+$/.test(version)) throw new Error(`bad Codex version ${JSON.stringify(version)}`);
  return { transcript: `transcript-${date}-${version}-herdr.jsonl`, pane: `pane-${date}-${version}-herdr.txt` };
}

// Not fixture-shaped, and never publishable: a run whose Codex version is not (yet) verified
// on the CLI, the daemon and the wire.
export function unverifiedNames(date) {
  return { transcript: `unverified-transcript-${date}-herdr.jsonl`, pane: `unverified-pane-${date}-herdr.txt` };
}

// --- pane text: scheduling and safety only -----------------------------------------------

// #303: the release-notes URL Codex's update prompt shows (codex-rs/tui/src/update_prompt.rs:42
// at rust-v0.160.0). Spelled in two parts so that oac-boundaries check 3 (no provider SDK name
// in the code tree, boundary-lint.yml) does not match a plain URL; it is no SDK use.
export const CODEX_RELEASE_NOTES_URL = `https://github.com/open${'a'}i/codex/releases/latest`;

// The kind table follows lib/g1.mjs DIALOG_KINDS (see the field notes there).
export const CODEX_DIALOG_KINDS = Object.freeze({
  'workspace-trust': {
    // Seen live (#199; 2026-09-30T17:28:35Z, driver run `probe4` of l3-beacon, read seq 48;
    // Codex CLI / app-server 0.159.2, Windows, TUI attached to the app-server daemon):
    //
    //     <cwd>
    //
    //     Note: You’re in a subdirectory of a Git project. Trusting will apply to the repository root:
    //     <root>
    //
    //     Trust this folder? Codex can read, edit, and run files here, subject to your permission …
    //     …
    //   › 1. Trust and continue
    //     2. Back to Agent Command Center
    //
    //     enter continue · esc back
    //
    // The Note block is optional body text above the detect line (Codex shows it when the trust
    // target differs from the cwd; on Windows a case-normalized spelling of the cwd itself does,
    // #199). The accepting option is preselected: the driver sends `enter` only. Codex's other
    // variants are not on record and are refused: "Quit" as option 2, "Open restricted" /
    // "Open existing task" as option 1, a different paragraph, or the footer "enter continue
    // and create sandbox · esc back" (Enter there also creates the Windows sandbox).
    // `body` and `note` are verbatim from codex-rs/tui/src/onboarding/trust_directory.rs at tag
    // rust-v0.159.2 (commit 8b9fa496bbf2c47aebd62e85a080b9a522a455b5, read 2026-09-30), and
    // match the capture; the capture shows the paragraph truncated with "…".
    detect: /^[ \t]*Trust this folder\?/m,
    acceptOption: /^Trust and continue$/i,
    options: Object.freeze(['Trust and continue', 'Back to Agent Command Center']),
    numbered: true,
    marker: '›',
    footer: /^[ \t]*enter continue · esc back[ \t]*\r?$/m,
    body: 'Trust this folder? Codex can read, edit, and run files here, subject to your permission settings. Folder settings can run code automatically, even without a model request. Continue only if you trust these files. Your trust decision will be saved.',
    note: 'Note: You’re in a subdirectory of a Git project. Trusting will apply to the repository root:',
    preselected: 0,
    accept: 0,
    verified: 'herdr L3 probe run probe4 2026-09-30 (#199), Codex CLI / app-server 0.159.2 on Windows; "1. Trust and continue" preselected',
  },
  'update-prompt': {
    // #303. Seen live (G4 herdr run 20261006T001351Z-5b2e11, driver c4def66, Codex CLI 0.160.0
    // on Windows; Codex pane capture, read seq 54 `codex:codex-startup-settled?`; the capture
    // was not committed, the run ended NOT RUN), verbatim:
    //
    //       Update available · 0.160.0 → 0.160.1
    //       Release notes: <CODEX_RELEASE_NOTES_URL>
    //
    //     › 1. Update now (runs `powershell -ExecutionPolicy Bypass -c '$env:CODEX_NON_INTERACTIVE=1; irm https://chatgpt.com/
    //          codex/install.ps1 | iex'`)
    //       2. Skip
    //       3. Skip until next version
    //
    //       enter continue · esc skip
    //
    // Source: codex-rs/tui/src/update_prompt.rs at tag rust-v0.160.0 (commit
    // a956835d020762cb2b570053af06f643a11c0ecc, read 2026-10-08): the title is "Update
    // available" · "<current> → <latest>" (:214-222), then "Release notes: " and the Codex
    // repository's releases/latest URL (CODEX_RELEASE_NOTES_URL below; :42, :229-230); the options are
    // `Update now (runs `{update_command}`)`, "Skip", "Skip until next version" (:243-247), the
    // highlight starts on "Update now" (:131), `down` moves it one option and wraps (:148,
    // :186-192), a digit key or Esc selects at once without moving (:149-151, :153; the
    // driver never sends either), Enter selects the highlighted option (:152). "Skip" (UpdateSelection::NotNow)
    // continues the launch and persists nothing (:94); "Skip until next version" writes the
    // updater's dismissal (updates::dismiss_version, :95-99); "Update now" runs the installer
    // (:90-93). The command in option 1 depends on how Codex was installed, so only its shape
    // is on record (`optionDetail`). The driver answers "2. Skip" only (#303, operator
    // decision): one `down` from the preselected "Update now", verified by a fresh read
    // showing exactly one `›` on "Skip", then Enter. It never selects option 1 or 3: a read
    // showing the highlight on either before Enter stops the run NOT RUN with no Enter sent.
    // Codex's NON-modal "✨ Update available! 0.160.0 -> 0.160.1" box (shown above the session
    // after a dismissal) is screen chrome, not this dialog, and does not match `detect`.
    detect: /^[ \t]*Update available · \S+ → \S+[ \t]*\r?$/m,
    acceptOption: /^Skip$/,
    options: Object.freeze(['Update now', 'Skip', 'Skip until next version']),
    optionDetail: Object.freeze([/^\(runs `[^`]+`\)$/, null, null]),
    numbered: true,
    marker: '›',
    footer: /^[ \t]*enter continue · esc skip[ \t]*\r?$/m,
    body: new RegExp(`^Update available · \\d+\\.\\d+\\.\\d+\\S* → \\d+\\.\\d+\\.\\d+\\S* Release notes: ${CODEX_RELEASE_NOTES_URL.replace(/[.*+?^${}()|[\]\\/]/g, '\\$&')}$`),
    preselected: 0,
    accept: 1,
    answer: '2. Skip',
    verified: 'herdr G4 run 20261006T001351Z-5b2e11, Codex CLI 0.160.0 on Windows, Codex pane read seq 54; "1. Update now" preselected (#303); codex-rs/tui/src/update_prompt.rs@rust-v0.160.0',
  },
  'hooks-review': {
    // Seen live (#204; 2026-09-30, scratch herdr runs of plain `codex` against the shared daemon,
    // Codex CLI / app-server 0.159.2, Windows), one untrusted user hook in hooks.json:
    //
    //     Hooks need review
    //     1 hook is new or changed.
    //     Hooks can run outside the sandbox after you trust them.
    //
    //   › 1. Review hooks
    //     2. Trust all and continue
    //     3. Continue without trusting (hooks won't run)
    //
    //     enter confirm · esc skip
    //
    // Source: codex-rs/tui/src/startup_hooks_review.rs at tag rust-v0.159.2 (read 2026-09-30):
    // shown when any hook's trust status is Untrusted or Modified (hooks_rpc.rs
    // hook_needs_review), after the app-server bootstrap and BEFORE App::run, so no thread is
    // started (no `thread/start`) until it is answered (codex-rs/tui/src/lib.rs). Trusting
    // writes `hooks.state.<key>.trusted_hash` into the Codex config (hooks_rpc.rs
    // write_hook_trusts). The driver never answers it: trusting a hook, or skipping it, is the
    // operator's decision (hooks run outside the sandbox; an L3 probe needs Beacon's hook to run).
    detect: /^[ \t]*Hooks need review[ \t]*\r?$/m,
    acceptOption: /(?!)/, // no option is the driver's to pick
    options: Object.freeze(['Review hooks', 'Trust all and continue', "Continue without trusting (hooks won't run)"]),
    footer: /^[ \t]*enter confirm · esc skip[ \t]*\r?$/m,
    refuse:
      "Codex's startup hook review is on screen (a hook in the Codex hooks config is new or changed, so untrusted); Codex starts no session until it is answered (codex-rs/tui/src/startup_hooks_review.rs@rust-v0.159.2). The driver never answers it: review and trust the hook(s) yourself first (in a Codex session of your own: `/hooks`, or this screen), then re-run; or run with accept=human and answer it during the run",
    verified: 'herdr scratch runs 2026-09-30 (#204), Codex CLI / app-server 0.159.2 on Windows; "1. Review hooks" preselected',
  },
  'mcp-tool-approval': {
    // #271 (operator decision 2026-10-04, a narrow exception to #197): Codex's MCP tool-approval
    // prompt, recorded as CODEX_TOOL_APPROVAL below. Detected by its form header ("Field 1/1")
    // so that a reworded question is still recognized, and refused with a precise reason. It is
    // NEVER planned by planDriverAccept (no `options`): driverMayAcceptCodex routes it to
    // planCodexToolApproval, which refuses it unless the scenario names the MCP server and
    // tools it registered itself (only g4-mcp-dual-era does), and then answers only "1. Allow".
    detect: /^[ \t]*Field \d+\/\d+[ \t]*\r?$/m,
    acceptOption: /^Allow(?:\s{2,}|$)/,
    options: null,
    footer: /^[ \t]*enter to submit \| esc to cancel[ \t]*\r?$/m,
    verified: 'herdr G4 run 2026-10-04 (20261004T050646Z), Codex CLI 0.160.0 on Windows, Codex pane section seq 58; "1. Allow" preselected (#271)',
  },
  'hooks-browser': {
    // Seen live (#204): what "1. Review hooks" (or Enter typed into the review) opens, with the
    // chat composer behind it. While it is open the TUI holds any startup submission
    // (startup_submission_has_protected_input: has_active_view, codex-rs/tui/src/chatwidget/
    // startup_submission.rs@rust-v0.159.2). Its footer, verbatim:
    //     t trust all · enter review · esc close
    detect: /^[ \t]*t trust all · enter review · esc close[ \t]*\r?$/m,
    acceptOption: /(?!)/,
    options: null,
    refuse: "Codex's hooks browser (`/hooks`) is open on the TUI and holds its input; trusting hooks is the operator's decision, so the driver does not close or answer it",
    verified: 'herdr scratch run 2026-09-30 (#204), Codex CLI / app-server 0.159.2 on Windows',
  },
});

// --- is the Codex TUI session ready for the operator's first message? (#204) --------------
//
// Codex 0.159.2 shows an editable composer ("› Ask Codex to do anything") BEFORE its session
// exists: the startup draft (codex-rs/tui/src/startup_draft.rs@rust-v0.159.2, "Keep the first
// composer editable and bottom-anchored while startup work continues"). Enter there only
// confirms the draft locally and shows "Waiting for startup · esc cancel"
// (startup_draft_input.rs); it is submitted once the session is configured and the gates in
// chatwidget/startup_submission.rs restore_startup_input_when_ready clear. Startup order
// (codex-rs/tui/src/lib.rs, app/startup.rs@rust-v0.159.2): draft composer -> app-server
// bootstrap -> startup hook review (if any hook needs review) -> App::run, which issues
// `thread/start` (spawn_startup_thread_start) -> SessionConfigured. So the composer on screen
// proves nothing; the driver types the operator's first message only when BOTH:
//   - wire: a thread not loaded before the launch is in the daemon's `thread/loaded/list`
//     (App::run has run and started a thread; submissions typed from then on are queued until
//     the session is configured: set_queue_submissions_until_session_configured), and
//   - pane: the idle composer placeholder, no dialog or startup screen, no in-progress
//     indicator and no "Waiting for startup" footer.
// The new thread is not proven to be the TUI's (another client of the shared daemon could load
// one); the marker's thread is still identified afterwards by preview + cwd (identifyTuiThread).
// The `thread/loaded/list` answer recorded after transcript line `sinceLine` (one poll's own
// lines): its data, or null when there is none or it is an error. Never an older entry.
export function loadedSince(loadedLists, sinceLine) {
  const l = (loadedLists ?? []).filter((x) => x.line > sinceLine).at(-1);
  return l && !l.error && Array.isArray(l.data) ? l.data : null;
}

// Findings a scenario records after the ready wait / on its expiry.
export const CODEX_NOT_ATTACHED_FINDING = 'the Codex TUI showed its composer but no thread new since the launch was loaded in the daemon before the ready wait ran out: the TUI may not have attached to the daemon (G2 criterion 1), or its session never started';
export const multipleNewThreadsFinding = (n) => `${n} threads new since the launch were loaded in the daemon when the Codex TUI became ready; another client of the shared daemon may have loaded one. The TUI's thread is still identified by the thread marker's preview and the project directory`;
export const codexReadyTimeoutFinding = (v) => (v?.composer && !v.newThreads?.length && !v.waitingForStartup ? CODEX_NOT_ATTACHED_FINDING : null);

export const CODEX_COMPOSER_IDLE = /^[ \t]*› Ask Codex to do anything[ \t]*\r?$/m;
export const CODEX_WAITING_FOR_STARTUP = /Waiting for startup/;

// -> { ready, composer, waitingForStartup, newThreads, why }
export function codexReadiness({ text, screen, loaded, preLoaded }) {
  const s = String(text ?? '');
  const composer = CODEX_COMPOSER_IDLE.test(s);
  const waitingForStartup = CODEX_WAITING_FOR_STARTUP.test(s);
  const pre = new Set(preLoaded ?? []);
  const newThreads = Array.isArray(loaded) ? loaded.filter((id) => !pre.has(id)) : [];
  let why = null;
  if (screen?.dialog) why = `a ${screen.dialog} screen is up`;
  else if (waitingForStartup) why = 'the TUI shows "Waiting for startup": a submission made during startup is held until the session is configured and its gates clear (codex-rs/tui/src/chatwidget/startup_submission.rs@rust-v0.159.2)';
  else if (!Array.isArray(loaded)) why = 'the daemon\'s `thread/loaded/list` could not be read';
  else if (composer && !newThreads.length) why = 'the TUI shows its composer but no thread new since the launch is loaded in the daemon (`thread/loaded/list`): either the session has not started (that composer is Codex\'s startup draft, before `thread/start`: codex-rs/tui/src/startup_draft.rs, app/startup.rs@rust-v0.159.2), or the TUI is not attached to this daemon (an embedded app-server; G2 criterion 1)';
  else if (!composer && newThreads.length) why = 'a new thread is loaded in the daemon but the TUI does not show its idle composer (another view or overlay holds its input)';
  else if (!composer) why = 'the TUI shows neither its composer nor a known startup screen, and no new thread is loaded in the daemon';
  else if (screen?.busy) why = 'the TUI shows the in-progress indicator';
  return { ready: !why, composer, waitingForStartup, newThreads, why };
}

// Poll the pane and the daemon until the TUI session is ready (codexReadiness), bounded by
// timeoutMs and the run's box. Dialogs go through the scenario's own handleDialog (the trust
// dialog is accepted, the hook review refused -> NOT RUN, or, under accept=human, waited for;
// time spent there does not count against timeoutMs). On expiry, stop() ends the run NOT RUN
// naming the last blocker; nothing is typed, so nothing is ever re-sent.
//   read(label, opts) -> classified read; listLoaded() -> string[] | null;
//   stop(reason) throws; returns { readSeq, newThreads, polls, waitedMs, observations }.
//   preLoaded must be the pre-launch baseline read from the pre-launch list's own lines: an
//   unreadable baseline (null) stops the run, because falling back to [] would count any thread
//   already loaded (e.g. by Codex Desktop) as new, a false "ready" (#205 review).
//   onTimeout(v) runs before the stop on expiry (e.g. to record a finding).
export async function waitCodexReady({ read, handleDialog, listLoaded, preLoaded, timeoutMs, pollMs, remainingMs, stop, onTimeout = null, sleep = (ms) => new Promise((r) => setTimeout(r, ms)), now = () => Date.now() }) {
  if (!Array.isArray(preLoaded)) stop('the daemon\'s pre-launch `thread/loaded/list` could not be read, so a thread new since the launch cannot be told apart; the Codex session\'s readiness cannot be verified and nothing is typed');
  const t0 = now();
  let deadline = t0 + Math.min(timeoutMs, Math.max(0, remainingMs()));
  const observations = [];
  let polls = 0;
  for (;;) {
    const r = await read('codex-ready-wait', { keep: 'on-change' });
    if (r.screen.dialog) {
      const h0 = now();
      await handleDialog(r, 'codex-ready-wait');
      deadline = Math.min(deadline + (now() - h0), now() + Math.max(0, remainingMs()));
      continue;
    }
    const loaded = await listLoaded();
    polls++;
    const v = codexReadiness({ text: r.text, screen: r.screen, loaded, preLoaded });
    const last = observations.at(-1);
    if (!last || last.why !== v.why) observations.push({ atMs: now() - t0, readSeq: r.seq, why: v.why, newThreads: v.newThreads.length });
    if (v.ready) return { readSeq: r.seq, newThreads: v.newThreads, polls, waitedMs: now() - t0, observations };
    if (now() + pollMs >= deadline) {
      onTimeout?.(v);
      stop(`the Codex TUI session was not ready within ${timeoutMs} ms of its startup (or the box ran out), so the thread marker was not typed: ${v.why}; nothing typed, nothing re-sent`);
    }
    await sleep(pollMs);
  }
}
// #303: "enter continue · esc skip" is the update prompt's footer; a screen showing it without
// the recorded title is an unrecognized dialog (refused at once), never a screen to wait on.
const GENERIC_DIALOG = /Press enter to (?:confirm|continue)|Enter to confirm|Esc to cancel|\(y\/n\)|Allow command\?|Approve\b.*\?|\benter continue\b.*\besc (?:back|skip)\b/i;

// -> { dialog: kind | 'unknown' | null, selected, options, busy }
export function classifyCodexScreen(text, { busyIndicator = 'esc to interrupt' } = {}) {
  const s = String(text ?? '');
  let dialog = null;
  for (const [kind, def] of Object.entries(CODEX_DIALOG_KINDS)) {
    if (def.detect.test(s)) {
      dialog = kind;
      break;
    }
  }
  if (!dialog && GENERIC_DIALOG.test(s)) dialog = 'unknown';
  const busy = busyIndicator ? s.toLowerCase().includes(busyIndicator.toLowerCase()) : false;
  // #271: the MCP tool-approval form is read with its own parser.
  if (dialog === 'mcp-tool-approval') return { dialog, variant: 'tool-approval', form: codexToolApprovalForm(s), selected: selectedOption(s), options: null, busy };
  if (dialog === 'update-prompt') return { dialog, updatePrompt: codexUpdateVersions(s), selected: selectedOption(s), options: dialogOptions(s, dialog, CODEX_DIALOG_KINDS), busy };
  return { dialog, selected: dialog ? selectedOption(s) : null, options: dialog ? dialogOptions(s, dialog, CODEX_DIALOG_KINDS) : null, busy };
}

// The driver accepts the Codex trust dialog on record (#199), under the same rules as Claude
// Code's (planDriverAccept): exactly its options, numbered and in order, one `›` selection, on
// option 1. #271: Codex's MCP tool-approval prompt is planned by planCodexToolApproval, and only
// when the scenario passes `toolApproval` (the server and tools it registered itself; only
// g4-mcp-dual-era does). Every other Codex dialog is refused: NOT RUN, no key sent.
export function driverMayAcceptCodex(classification, { toolApproval = null } = {}) {
  if (classification?.dialog === 'mcp-tool-approval') return planCodexToolApproval(classification, toolApproval);
  if (classification?.dialog === 'update-prompt') return planCodexUpdateSkip(classification);
  return planDriverAccept(classification, CODEX_DIALOG_KINDS);
}

// --- #303: Codex's start-up update prompt ---------------------------------------------------
//
// The versions the prompt names ("Update available · <current> → <latest>"), or nulls.
export function codexUpdateVersions(text) {
  const m = /^[ \t]*Update available · (\S+) → (\S+)[ \t]*\r?$/m.exec(String(text ?? ''));
  return { current: m?.[1] ?? null, latest: m?.[2] ?? null };
}

// The driver answers the recorded prompt "2. Skip" (planDriverAccept on the kind on record:
// one `down` from "1. Update now", verified, then Enter). Anything off record ends the run NOT
// RUN at once, on the first read of the prompt, with no key sent: the run never waits out a
// handshake or attach timeout behind it. The plan carries `updatePrompt` (the versions shown
// and, once sent, the answer), recorded on the dialog.
export function planCodexUpdateSkip(classification) {
  const p = planDriverAccept(classification, CODEX_DIALOG_KINDS);
  const v = classification?.updatePrompt ?? { current: null, latest: null };
  const updatePrompt = { current: v.current, latest: v.latest, answer: null };
  if (p.ok) return { ...p, answer: CODEX_DIALOG_KINDS['update-prompt'].answer, updatePrompt };
  return { ...p, updatePrompt, why: codexUpdatePromptStop(v, p.why) };
}
export const codexUpdatePromptStop = (v, why) =>
  `Codex update prompt shown at start-up (Codex ${v.current ?? '?'} → ${v.latest ?? '?'}), not in the form on record (${why}); answer it in Codex's own TUI ("2. Skip" applies to that launch only), then re-run. The driver never runs an update and never writes Codex's updater state`;
// A driverMayAcceptCodex bound to a scenario's own tool-approval expectation, read when a
// dialog is planned. getExpected() -> { server, tools, arguments } | null.
export const driverMayAcceptCodexExpecting = (getExpected) => (classification) => driverMayAcceptCodex(classification, { toolApproval: getExpected() ?? null });

// --- #271: Codex's MCP tool-approval prompt ----------------------------------------------------
//
// Operator decision on #271 (2026-10-04), a narrow exception to #197: the driver MAY answer this
// prompt, only in the G4 scenario, only for the MCP server and tools that scenario registered
// itself (from its committed config, lib/g4.mjs g4CodexToolApproval; never from the pane), and
// only with "1. Allow" (this call only). Recorded verbatim from the live G4 herdr run
// 20261004T050646Z (Codex CLI 0.160.0, Windows; Codex pane capture, section seq 58):
//
//       Field 1/1
//       Allow the g4http MCP server to run tool "g4_echo"?
//
//       text: hello from codex through herdr
//
//       › 1. Allow                   Run the tool and continue
//         2. Allow for this session  Run the tool and remember this choice for this session
//         3. Always allow            Run the tool and remember this choice for future tool calls
//         4. Cancel                  Cancel this tool call
//       enter to submit | esc to cancel
//
// Source (the Codex repository at tag rust-v0.160.0, commit a956835d020762cb2b570053af06f643a11c0ecc,
// read 2026-10-04): the question is `Allow {actor} to run tool "{tool_name}"?` with actor `the
// {server} MCP server` (codex-rs/core/src/mcp_tool_call.rs:1961-1977,
// build_mcp_tool_approval_fallback_message); the TUI options and descriptions come from
// codex-rs/tui/src/bottom_pane/mcp_server_elicitation.rs:251-291, with option 1 the default
// selection (:307-315, :765). "Allow" submits an elicitation Accept with no meta and no content
// (submit_answers, :1155-1178), which parse_mcp_tool_approval_elicitation_response maps to
// ReviewDecision::Approved (mcp_tool_call.rs:2131-2167, line 2160), and
// apply_mcp_tool_approval_decision (:2258-2285) does nothing for Approved: nothing is remembered
// for the session and nothing is written to config. "Allow for this session" remembers the
// approval for the session; "Always allow" persists it (maybe_persist_mcp_tool_approval, :2287).
// The driver never selects either. That no persistent approval was written is also checked per
// run: just before the first driver Allow, run.mjs hashes the harness config and requires the
// teardown hashes to equal that snapshot (ctx.requireHarnessConfigUnchanged).
//
// The lines between the question and the options are the tool's arguments as Codex displays
// them (`<name>: <value>`, one line each); each name must be one of the tool's declared inputs.
export const CODEX_TOOL_APPROVAL = Object.freeze({
  field: 'Field 1/1',
  question: /^Allow the ([A-Za-z0-9_-]+) MCP server to run tool "([A-Za-z0-9_-]+)"\?$/,
  argument: /^([A-Za-z_][A-Za-z0-9_]*): (\S.*)$/,
  options: Object.freeze([
    Object.freeze(['Allow', 'Run the tool and continue']),
    Object.freeze(['Allow for this session', 'Run the tool and remember this choice for this session']),
    Object.freeze(['Always allow', 'Run the tool and remember this choice for future tool calls']),
    Object.freeze(['Cancel', 'Cancel this tool call']),
  ]),
  marker: '›',
  preselected: 0,
  accept: 0,
  answer: '1. Allow',
  verified: CODEX_DIALOG_KINDS['mcp-tool-approval'].verified,
});

const TA_FIELD = /^[ \t]*Field \d+\/\d+[ \t]*$/;
const TA_FOOTER = /^[ \t]*enter to submit \| esc to cancel[ \t]*$/;
const TA_OPTION = /^\s*([❯›▶▸→>*])?\s*(\d+)\.\s+(\S.*?)\s*$/;
const TA_ANY_MARK = /^\s*[❯›▶▸→]/;

// The tool-approval form as the pane shows it, or null when no form header is on screen.
// -> { field, question, server, tool, arguments: [{ name, value }], options: [{ number, label,
// description, selected, mark }], marked, footer, unknown } where `unknown` lists every line
// from the header to the footer that is not of the recorded shape (and '(…)' notes). Lines
// above the header are the session transcript (the operator's prompt, "• Calling …") and are
// not part of the form; a selection marker below the footer counts.
export function codexToolApprovalForm(text, v = CODEX_TOOL_APPROVAL) {
  const lines = String(text ?? '').replace(/\r/g, '').split('\n');
  const at = lines.findIndex((l) => TA_FIELD.test(l));
  if (at === -1) return null;
  const form = { field: lines[at].trim(), question: null, server: null, tool: null, arguments: [], options: [], marked: 0, footer: false, unknown: [] };
  if (lines.filter((l) => TA_FIELD.test(l)).length !== 1) form.unknown.push('(more than one form header on screen)');
  if (form.field !== v.field) form.unknown.push(`(form header ${JSON.stringify(form.field)} off record)`);
  const foot = lines.findIndex((l, i) => i > at && TA_FOOTER.test(l));
  form.footer = foot !== -1;
  if (!form.footer) form.unknown.push('(recorded footer not on screen)');
  const region = lines.slice(at + 1, form.footer ? foot : lines.length);
  let i = 0;
  while (i < region.length && !region[i].trim()) i += 1;
  if (i < region.length) {
    form.question = region[i].trim();
    const m = v.question.exec(form.question);
    if (m) [form.server, form.tool] = [m[1], m[2]];
    else form.unknown.push('(question text off record)');
    i += 1;
  } else form.unknown.push('(no question on screen)');
  for (; i < region.length && !TA_OPTION.test(region[i]); i += 1) {
    const t = region[i].trim();
    if (!t) continue;
    const a = v.argument.exec(t);
    if (a) form.arguments.push({ name: a[1], value: a[2] });
    else form.unknown.push(t);
  }
  for (; i < region.length; i += 1) {
    if (!region[i].trim()) continue;
    const o = TA_OPTION.exec(region[i]);
    if (!o) {
      form.unknown.push(region[i].trim());
      continue;
    }
    const [, mark, num, rest] = o;
    // `>` and `*` count as selection markers only before a number, as in lib/g1.mjs.
    const [label, ...desc] = rest.split(/\s{2,}/);
    form.options.push({ number: Number(num), label, description: desc.join('  ') || null, selected: !!mark, mark: mark ?? null });
    if (mark) form.marked += 1;
  }
  if (form.footer) for (const l of lines.slice(foot + 1)) if (TA_ANY_MARK.test(l)) form.marked += 1;
  return form;
}

// The shape of a form's options compared with the record: null, or why not.
function taOptionsOffRecord(f, v) {
  const shown = f.options.map((o) => [o.label, o.description]);
  if (JSON.stringify(shown) !== JSON.stringify(v.options)) return `the options on screen ${JSON.stringify(f.options.map((o) => `${o.number}. ${o.label}`))} are not the ones on record ${JSON.stringify(v.options.map(([l], n) => `${n + 1}. ${l}`))}`;
  if (f.options.some((o, n) => o.number !== n + 1)) return `the options are numbered ${JSON.stringify(f.options.map((o) => o.number))}, not 1-${v.options.length} as on record`;
  return null;
}

// How may the DRIVER answer the tool-approval prompt? Same result shape as planDriverAccept,
// plus `toolApproval` (what the prompt asked and what the scenario expected; recorded on the
// dialog, refused or not), `answer`, and `confirm` (the check a fresh read must pass before
// Enter). expected: { server, tools: string[], arguments: { <tool>: string[] } } from the
// scenario's own committed config, or null (every scenario but G4): refused.
export function planCodexToolApproval(classification, expected, v = CODEX_TOOL_APPROVAL) {
  const f = classification?.form ?? null;
  const exp = expected && typeof expected.server === 'string' && Array.isArray(expected.tools) && expected.tools.length ? { server: expected.server, tools: [...expected.tools], arguments: expected.arguments ?? null } : null;
  const toolApproval = { field: f?.field ?? null, question: f?.question ?? null, server: f?.server ?? null, tool: f?.tool ?? null, arguments: f?.arguments ?? [], expected: exp, answer: null };
  const no = (why) => ({ ok: false, why: `Codex's MCP tool-approval prompt: ${why}; the driver does not answer it (#197 stands; #271 lets only the G4 scenario allow its own registered server's tools)`, moves: [], keys: [], toolApproval });
  if (!f) return no('no form read');
  if (!exp) return no('this scenario registered no MCP server and tools the driver may allow');
  if (f.unknown.length) return no(`text off record on screen (${JSON.stringify(f.unknown)})`);
  if (f.server !== exp.server) return no(`the server ${JSON.stringify(f.server)} is not the one this scenario registered (${JSON.stringify(exp.server)})`);
  if (!exp.tools.includes(f.tool)) return no(`the tool ${JSON.stringify(f.tool)} is not one this scenario registered (${JSON.stringify(exp.tools)})`);
  const declared = exp.arguments?.[f.tool];
  const names = f.arguments.map((a) => a.name);
  if (!Array.isArray(declared)) return no(`no declared inputs recorded for ${JSON.stringify(f.tool)}`);
  if (names.some((n) => !declared.includes(n)) || new Set(names).size !== names.length) return no(`the arguments shown ${JSON.stringify(names)} are not the tool's declared inputs ${JSON.stringify(declared)}, each at most once`);
  const off = taOptionsOffRecord(f, v);
  if (off) return no(off);
  if (f.marked !== 1) return no(`${f.marked} selection markers on screen, not exactly one`);
  const sel = f.options.findIndex((o) => o.selected);
  if (sel === -1) return no('no option is shown selected (the one marker is outside the options)');
  if (f.options[sel].mark !== v.marker) return no(`the selection marker is ${JSON.stringify(f.options[sel].mark)}, not the ${JSON.stringify(v.marker)} on record`);
  if (sel !== v.accept) return no(`the selected option is "${sel + 1}. ${f.options[sel].label}", not "${v.answer}"; the driver never moves the selection here and never answers "Allow for this session" or "Always allow"`);
  const verify = { field: f.field, question: f.question, server: f.server, tool: f.tool, arguments: f.arguments };
  return { ok: true, why: null, moves: [], keys: ['enter'], answer: v.answer, toolApproval, confirm: (screen) => codexToolApprovalCheck(screen, verify, v) };
}

// The fresh read before Enter (#271, as for the Claude multi-select form): the same prompt
// (header, question, server, tool, arguments), the options on record, exactly one marker, the
// recorded `›`, on "1. Allow". -> { state: 'ok' } | { state: 'wait', why } | { state: 'stop', why }.
export function codexToolApprovalCheck(screen, verify, v = CODEX_TOOL_APPROVAL) {
  const f = screen?.variant === 'tool-approval' ? screen.form : null;
  if (!f) return { state: 'wait', why: 'no tool-approval form read' };
  if (f.unknown.length) return { state: 'wait', why: `text off record on screen (${JSON.stringify(f.unknown)})` };
  if (f.field !== verify.field || f.question !== verify.question || f.server !== verify.server || f.tool !== verify.tool || JSON.stringify(f.arguments) !== JSON.stringify(verify.arguments)) return { state: 'stop', why: `the prompt on screen (${JSON.stringify(f.question)}, ${JSON.stringify(f.arguments)}) is not the one planned from` };
  const off = taOptionsOffRecord(f, v);
  if (off) return { state: 'stop', why: off };
  if (f.marked !== 1) return { state: 'wait', why: `${f.marked} selection markers on screen, not exactly one` };
  const sel = f.options.findIndex((o) => o.selected);
  if (sel === -1) return { state: 'stop', why: 'no option is shown selected' };
  if (f.options[sel].mark !== v.marker) return { state: 'stop', why: `the selection marker is ${JSON.stringify(f.options[sel].mark)}, not ${JSON.stringify(v.marker)}` };
  if (sel !== v.accept) return { state: 'stop', why: `the selection is "${sel + 1}. ${f.options[sel].label}", not "${v.answer}"` };
  return { state: 'ok' };
}
export { normalizeDialogText };

// --- the pane's process argv (the "plain codex" proof) ------------------------------------

// #232: the pane's process argv is read from the run's one process-table snapshot
// (lib/proc.mjs processTable: Linux /proc/<pid>/cmdline, macOS `ps`, Windows one
// Win32_Process query), never one OS query per pid (~1.6 s each on Windows). Never the
// process environment. The FULL argv stays in memory; only paneArgv's minimized projection
// below is ever recorded.

// One pid's argv from a process-table snapshot: { pid, argv, commandLine, platform, source },
// `argv` an array where the OS gives one (Linux), else null with `commandLine` as the OS prints
// it. `platform` is the row's own (set by lib/proc.mjs's table parsers), or null for a row
// without one: a consumer then splits with ITS fallback (#249), so a row's stored platform wins
// and an untagged row really falls back. `platform` here is that fallback, used only to name
// `source` by the platform whose rules will actually split the row.
// Unminimized: in memory only, never put it in a record.
export function processArgv(pid, table, { platform: fallback = process.platform } = {}) {
  if (!Number.isInteger(pid) || pid <= 0) return { pid, argv: null, commandLine: null, platform: null, source: 'no pid' };
  if (!table) return { pid, argv: null, commandLine: null, platform: null, source: `process table not readable on ${process.platform}` };
  const p = table.get(pid);
  if (!p) return { pid, argv: null, commandLine: null, platform: null, source: 'not in the process table (process gone?)' };
  if (Array.isArray(p.argv)) return { pid, argv: [...p.argv], commandLine: null, platform: p.platform ?? null, source: `/proc/${pid}/cmdline (process table)` };
  const used = p.platform ?? fallback;
  const source = used === 'win32' ? 'Win32_Process.CommandLine (process table)' : used === 'linux' && p.commandLine == null ? `/proc/${pid}/cmdline not readable (process table)` : 'ps command (process table)';
  return { pid, argv: null, commandLine: typeof p.commandLine === 'string' ? p.commandLine : null, platform: p.platform ?? null, source };
}

// --- minimized argv (#232) ------------------------------------------------------------------
//
// A pane descendant's argv can carry a secret no redaction pattern knows (a token passed as
// an argument). Records keep only what launch identity needs: the executable's basename, the
// basename of a `codex` token, and the exact arguments the scenario itself asserts (its
// allowlist: none for a plain `codex` launch, the validated `-c` overrides for G4). Every
// other argument becomes `<arg len=N>`, so the count of arguments (what "plain" asserts)
// survives and nothing of the value does. Length only, no hash: an unkeyed hash of a short
// secret is an offline brute-force oracle; a per-run keyed hash would only show equality
// within one run, which no check uses, and its key would be one more secret to keep out of
// the record. The run manifest still goes through the fail-closed redaction scan
// (redactValue + withholdResiduals in run.mjs) on top of this.

export const argPlaceholder = (value) => `<arg len=${String(value).length}>`;
export const arg0Placeholder = (value) => `<arg0 len=${String(value).length}>`;
const tokenBase = (t) => basename(String(t).replace(/\\/g, '/'));

// #244 (PR #242 review note C): argv[0] is whatever the process put there. On Linux a process
// can rewrite its own cmdline (node's process.title, setproctitle: `sshd: user@pts/0`), so its
// "basename" is process-chosen text, not necessarily an executable name. argv[0] is kept (as
// its basename) only when that basename is one of the executables a harness pane is expected
// to run: a shell (a login shell's leading `-` allowed), the node runtime, a harness CLI, or
// herdr, optionally with a Windows/script extension. Anything else becomes `<arg0 len=N>`.
// Default-deny on purpose: an unknown helper only loses its name in the record; no check reads
// an argv[0] (the launch proof runs on the full argv in memory and keeps the `codex` token
// through CODEX_TOKEN, not through this list).
export const EXPECTED_EXECUTABLE = /^-?(?:sh|bash|dash|zsh|fish|ksh|pwsh|powershell|cmd|node|nodejs|codex|claude|herdr)(?:\.exe|\.cmd|\.bat|\.ps1|\.js)?$/i;

// tokens: a full argv. allow: exact argument strings the scenario asserts. executable: the
// first token is the executable (its basename kept when EXPECTED_EXECUTABLE, else a
// placeholder). null in, null out.
export function minimizeArgv(tokens, { allow = [], executable = true } = {}) {
  if (!Array.isArray(tokens)) return null;
  const allowed = new Set(allow.map(String));
  return tokens.map((t, i) => {
    const s = String(t);
    if (CODEX_TOKEN.test(tokenBase(s))) return tokenBase(s);
    if (executable && i === 0) return EXPECTED_EXECUTABLE.test(tokenBase(s)) ? tokenBase(s) : arg0Placeholder(s);
    return allowed.has(s) ? s : argPlaceholder(s);
  });
}

// The recordable view of a pane's process tree: the first `limit` pids' argv read from one
// process-table snapshot, minimized, and the `codex` launch proof computed on the FULL argv
// in memory, its argsAfterCodex minimized the same way. expectArgsAfterCodex (G4): the exact
// arguments the launch must carry; `matchesExpected` compares them on the full argv.
// platform (#249): a fallback only. Each record's `commandLine` is split by the rules of the
// platform its row was read on (record.platform, from processArgv), the same per-row rule
// lib/proc.mjs commandTokens() applies at teardown; `platform` is used for a record without one.
export function paneArgv(pids, table, { allow = [], expectArgsAfterCodex = null, limit = 32, platform = process.platform } = {}) {
  const full = pids.slice(0, limit).map((pid) => processArgv(pid, table, { platform }));
  const p = codexLaunchProof(full, { platform });
  const proof = { ...p, argsAfterCodex: minimizeArgv(p.argsAfterCodex, { allow, executable: false }) };
  if (expectArgsAfterCodex) proof.matchesExpected = p.found ? JSON.stringify(p.argsAfterCodex) === JSON.stringify(expectArgsAfterCodex) : null;
  const argv = full.map((r) => ({ pid: r.pid, argv: minimizeArgv(recordTokens(r, platform), { allow }), source: r.source, minimized: true }));
  return { argv, proof };
}

// A record's tokens: its argv, else its command line split under the record's own platform
// (the fallback for a record that carries none). null when there is neither.
function recordTokens(r, platform) {
  if (Array.isArray(r?.argv)) return r.argv;
  return r?.commandLine != null ? splitCommandLine(r.commandLine, { platform: r.platform ?? platform }) : null;
}

// splitCommandLine / splitWindowsCommandLine (#243) live in lib/proc.mjs (#244), so teardown's
// commandTokens() and the launch proof split a command line by one rule set; re-exported here.

const CODEX_TOKEN = /^codex(?:\.js|\.exe|\.cmd|\.ps1)?$/i;

// Given argv records (herdr's foreground processes first, then their descendants), find the
// first process running `codex` and what came after the `codex` token. `plain` is true only
// when that process has no argument after it. A record's command line is split under its own
// platform (record.platform; `platform` is the fallback for a record without one, #249).
export function codexLaunchProof(records, { platform = process.platform } = {}) {
  for (const r of records) {
    const tokens = recordTokens(r, platform);
    if (!tokens) continue;
    const i = tokens.findIndex((t) => CODEX_TOKEN.test(basename(String(t).replace(/\\/g, '/'))));
    if (i === -1) continue;
    const after = tokens.slice(i + 1);
    return { found: true, pid: r.pid, codexToken: basename(String(tokens[i]).replace(/\\/g, '/')), argsAfterCodex: after, plain: after.length === 0 };
  }
  return { found: false, pid: null, codexToken: null, argsAfterCodex: null, plain: false };
}

// --- the client's wire transcript -----------------------------------------------------------
//
// Lines are the committed client's own records: { t, mode, direction, payload }, direction
// one of handshake, client->daemon, daemon->client, proxy-stderr, ws-close, unparsed. Several
// client processes append to one file; each process run is one connection, begun by its
// `handshake` line. Two processes of the same mode never overlap in this scenario, so a
// connection is (mode, the handshake that opened it).

export function parseG2Transcript(text, { completeLinesOnly = false } = {}) {
  let s = String(text ?? '');
  if (completeLinesOnly) s = s.slice(0, s.lastIndexOf('\n') + 1);
  const out = [];
  s.split('\n').forEach((raw, i) => {
    if (!raw.trim()) return;
    let e;
    try {
      e = JSON.parse(raw);
    } catch {
      throw new Error(`G2 transcript line ${i + 1} is not JSON`);
    }
    out.push({ line: i + 1, t: e.t, mode: e.mode, direction: e.direction, payload: e.payload });
  });
  return out;
}

const textOf = (input) => (Array.isArray(input) ? input.map((x) => (typeof x?.text === 'string' ? x.text : '')).join('') : '');
const statusType = (s) => (typeof s === 'string' ? s : s?.type ?? null);
const cwdsOf = (th) => [...new Set([th?.cwd, ...(Array.isArray(th?.environments) ? th.environments.map((e) => e?.cwd) : [])].filter((x) => typeof x === 'string'))];

// Tag every entry with its connection and, for daemon responses, the method answered.
export function annotateConnections(entries) {
  const conns = [];
  const current = new Map(); // mode -> connection
  for (const e of entries) {
    let c = current.get(e.mode);
    if (e.direction === 'handshake' || !c) {
      c = { index: conns.length, mode: e.mode, firstLine: e.line, lastLine: e.line, upgraded: null, handshake: null, requests: new Map(), userAgent: null, userAgentVersion: null, errors: [], unparsed: 0 };
      conns.push(c);
      current.set(e.mode, c);
    }
    c.lastLine = e.line;
    e.conn = c.index;
    if (e.direction === 'handshake') {
      c.handshake = String(e.payload ?? '').split('\r\n')[0];
      c.upgraded = /^HTTP\/1\.1 101/.test(c.handshake);
    } else if (e.direction === 'unparsed') c.unparsed += 1;
    const p = e.payload;
    if (e.direction === 'client->daemon' && p && p.id !== undefined && p.method) c.requests.set(p.id, { method: p.method, params: p.params, line: e.line, t: e.t });
    if (e.direction === 'daemon->client' && p && p.id !== undefined && !p.method && c.requests.has(p.id)) {
      const req = c.requests.get(p.id);
      e.answers = req.method;
      e.request = req;
      if (p.error) c.errors.push({ line: e.line, method: req.method, error: p.error });
      if (req.method === 'initialize' && p.result) {
        c.userAgent = p.result.userAgent ?? null;
        const m = /^[^\s/]+\/v?(\d+\.\d+\.\d+)(?=$|[\s(])/.exec(String(c.userAgent ?? ''));
        c.userAgentVersion = m ? m[1] : null;
      }
    }
  }
  return conns.map(({ requests, ...rest }) => ({ ...rest, requestMethods: [...requests.values()].map((r) => r.method) }));
}

// Everything the scenario and the report read from a G2 transcript.
export function g2Facts(entries) {
  const es = entries.map((e) => ({ ...e }));
  const connections = annotateConnections(es);
  const f = {
    connections,
    loadedLists: [],
    threadLists: [],
    resumes: [],
    turnStarts: [],
    queueAdds: [],
    turnsLists: [],
    events: { turnStarted: [], turnCompleted: [], userItems: [], agentMessages: [], statusChanged: [] },
    firstT: es[0]?.t ?? null,
    lastT: es.at(-1)?.t ?? null,
  };
  for (const e of es) {
    const p = e.payload;
    if (e.direction !== 'daemon->client' || !p || typeof p !== 'object') continue;
    const base = { line: e.line, t: e.t, mode: e.mode, conn: e.conn };
    if (e.answers) {
      const req = e.request;
      const err = p.error ?? null;
      if (e.answers === 'thread/loaded/list') f.loadedLists.push({ ...base, data: Array.isArray(p.result?.data) ? p.result.data : [], error: err });
      else if (e.answers === 'thread/list') f.threadLists.push({ ...base, entries: (p.result?.data ?? []).map((th) => ({ id: th?.id, preview: th?.preview ?? null, cwds: cwdsOf(th) })), fixtureNote: p.result?._fixtureNote ?? null, error: err });
      else if (e.answers === 'thread/resume') f.resumes.push({ ...base, reqLine: req.line, threadId: req.params?.threadId, status: statusType(p.result?.thread?.status), error: err });
      else if (e.answers === 'turn/start') f.turnStarts.push({ ...base, reqLine: req.line, reqT: req.t, threadId: req.params?.threadId, text: textOf(req.params?.input), turnId: p.result?.turn?.id ?? null, status: p.result?.turn?.status ?? null, error: err });
      else if (e.answers === 'thread/queue/add') f.queueAdds.push({ ...base, reqLine: req.line, reqT: req.t, threadId: req.params?.threadId, clientUserMessageId: req.params?.clientUserMessageId ?? null, text: textOf(req.params?.input), queuedId: p.result?.queuedSubmission?.id ?? null, error: err });
      else if (e.answers === 'thread/turns/list') {
        f.turnsLists.push({
          ...base,
          reqLine: req.line,
          threadId: req.params?.threadId,
          error: err,
          turns: (p.result?.data ?? []).map((tn) => ({
            id: tn?.id,
            status: tn?.status ?? null,
            startedAt: tn?.startedAt ?? null,
            completedAt: tn?.completedAt ?? null,
            userTexts: (tn?.items ?? []).filter((it) => it?.type === 'userMessage').map((it) => textOf(it.content)),
            userClientIds: (tn?.items ?? []).filter((it) => it?.type === 'userMessage').map((it) => it.clientId ?? null),
            agentMessages: (tn?.items ?? []).filter((it) => it?.type === 'agentMessage').map((it) => it.text ?? ''),
          })),
        });
      }
      continue;
    }
    const prm = p.params ?? {};
    if (p.method === 'turn/started') f.events.turnStarted.push({ ...base, threadId: prm.threadId, turnId: prm.turn?.id ?? null });
    else if (p.method === 'turn/completed') f.events.turnCompleted.push({ ...base, threadId: prm.threadId, turnId: prm.turn?.id ?? null, status: prm.turn?.status ?? null, agentMessages: (prm.turn?.items ?? []).filter((it) => it?.type === 'agentMessage').map((it) => it.text ?? '') });
    else if ((p.method === 'item/started' || p.method === 'item/completed') && prm.item?.type === 'userMessage') f.events.userItems.push({ ...base, method: p.method, threadId: prm.threadId, turnId: prm.turnId ?? null, clientId: prm.item.clientId ?? null, text: textOf(prm.item.content) });
    else if (p.method === 'item/completed' && prm.item?.type === 'agentMessage') f.events.agentMessages.push({ ...base, threadId: prm.threadId, turnId: prm.turnId ?? null, text: prm.item.text ?? '' });
    else if (p.method === 'thread/status/changed') f.events.statusChanged.push({ ...base, threadId: prm.threadId, type: statusType(prm.status) });
  }
  return f;
}

// The TUI's thread: listed in thread/list with the operator's prompt as its preview and the
// scratch project directory as its cwd, and loaded in the daemon (thread/loaded/list).
// originator/source are deliberately NOT used (G2-result.md: they track the
// first-connecting client, not the thread's creator). -> { threadId, candidates, why }
export function identifyTuiThread(facts, { operatorPrompt, projectDirs, sinceLine = 0 }) {
  const loaded = new Set(facts.loadedLists.filter((l) => l.line > sinceLine).flatMap((l) => l.data));
  const norm = (p) => String(p).replace(/\\/g, '/').replace(/\/+$/, '');
  const dirs = new Set(projectDirs.map(norm));
  const want = String(operatorPrompt).trim();
  const cands = new Map();
  for (const tl of facts.threadLists.filter((l) => l.line > sinceLine)) {
    for (const th of tl.entries) {
      const previewOk = typeof th.preview === 'string' && th.preview.trim().length > 0 && (th.preview.trim() === want || want.startsWith(th.preview.trim().replace(/…$|\.\.\.$/, '')));
      const cwdOk = th.cwds.some((c) => dirs.has(norm(c)));
      if (previewOk && cwdOk && loaded.has(th.id)) cands.set(th.id, { id: th.id, listLine: tl.line });
    }
  }
  const list = [...cands.values()];
  if (list.length === 1) return { threadId: list[0].id, candidates: list, why: null };
  return { threadId: null, candidates: list, why: list.length ? 'more than one loaded thread matches the operator prompt and project directory' : 'no loaded thread matches the operator prompt and project directory yet' };
}

// --- fixture sanitizer ---------------------------------------------------------------------
//
// What the human runs' redaction did to the committed G2 fixtures (G2-result.md "Redaction"),
// done structurally here before the generic redaction in run.mjs:
//   - thread/list results keep only the run's own TUI thread (other saved sessions carry
//     private prompt previews and rollout paths) and are marked with _fixtureNote;
//   - remoteControl/status/changed serverName -> <HOST>, installationId -> <INSTALLATION_ID>;
//   - account plan and credit fields -> <REDACTED>.
// Returns the sanitized text, counts, and the installation ids seen (for the caller to also
// register as run literals; they are never recorded themselves).
export function sanitizeTranscript(text, { ownThreadId = null } = {}) {
  const lines = String(text ?? '').split('\n');
  const report = { threadListEntriesRemoved: 0, threadListResultsTouched: 0, serverNames: 0, installationIds: 0, planFields: 0, creditFields: 0, unparsedLines: 0 };
  const installationIds = new Set();
  const pending = new Map(); // `${mode}\0${id}` -> method (per open connection of a mode)
  const out = lines.map((raw) => {
    if (!raw.trim()) return raw;
    let e;
    try {
      e = JSON.parse(raw);
    } catch {
      report.unparsedLines += 1;
      return raw; // left for run.mjs's fail-closed redaction and residual scan
    }
    const p = e.payload;
    if (e.direction === 'handshake') for (const k of [...pending.keys()]) if (k.startsWith(`${e.mode}\0`)) pending.delete(k);
    if (e.direction === 'client->daemon' && p?.id !== undefined && p.method) pending.set(`${e.mode}\0${p.id}`, p.method);
    if (e.direction === 'daemon->client' && p && typeof p === 'object') {
      const answered = p.id !== undefined && !p.method ? pending.get(`${e.mode}\0${p.id}`) : null;
      if (answered === 'thread/list' && Array.isArray(p.result?.data)) {
        const keep = p.result.data.filter((th) => ownThreadId && th?.id === ownThreadId);
        const removed = p.result.data.length - keep.length;
        if (removed) {
          p.result.data = keep;
          p.result._fixtureNote = 'unrelated sessions removed for privacy';
          report.threadListEntriesRemoved += removed;
          report.threadListResultsTouched += 1;
        }
      }
      const prm = p.params;
      if (p.method === 'remoteControl/status/changed' && prm) {
        if (typeof prm.serverName === 'string' && prm.serverName !== '<HOST>') {
          prm.serverName = '<HOST>';
          report.serverNames += 1;
        }
        if (typeof prm.installationId === 'string' && prm.installationId !== '<INSTALLATION_ID>') {
          installationIds.add(prm.installationId);
          prm.installationId = '<INSTALLATION_ID>';
          report.installationIds += 1;
        }
      }
      if (p.method === 'account/updated' && prm && prm.planType != null && prm.planType !== '<REDACTED>') {
        prm.planType = '<REDACTED>';
        report.planFields += 1;
      }
      if (p.method === 'account/rateLimits/updated' && prm?.rateLimits) {
        for (const k of ['planType', 'credits']) {
          if (prm.rateLimits[k] != null && prm.rateLimits[k] !== '<REDACTED>') {
            prm.rateLimits[k] = '<REDACTED>';
            report[k === 'credits' ? 'creditFields' : 'planFields'] += 1;
          }
        }
      }
    }
    return JSON.stringify(e);
  });
  return { text: out.join('\n'), report, installationIds: [...installationIds] };
}

// --- method-sequence comparison, one client connection against another ------------------

// Notifications the daemon pushes to every connection regardless of what the client did.
export const AMBIENT_NOTIFICATIONS = new Set(['remoteControl/status/changed', 'account/updated', 'account/rateLimits/updated', 'thread/tokenUsage/updated']);

export function connectionKeys(entries, connIndex, { ambient = false } = {}) {
  return entries
    .filter((e) => e.conn === connIndex)
    .filter((e) => ambient || !AMBIENT_NOTIFICATIONS.has(e.payload?.method))
    .map((e) => {
      const p = e.payload;
      if (e.direction === 'handshake') return `handshake ${String(p ?? '').split('\r\n')[0]}`;
      if (e.direction === 'client->daemon') return `c->d ${p?.id !== undefined ? 'request' : 'notification'} ${p?.method}`;
      if (e.direction === 'daemon->client') {
        if (p?.method) return `d->c ${p.id !== undefined ? 'server-request' : 'notification'} ${p.method}`;
        return `d->c ${p?.error ? `error ${p.error.code}` : 'result'} (${e.answers ?? '?'})`;
      }
      return e.direction;
    });
}

// For each baseline connection mode (list, turn, busyqueue, turns), diff the baseline's
// connection of that mode against the run's last connection of that mode, ambient
// notifications left out.
export function compareByMode(baselineEntries, runEntries) {
  const b = baselineEntries.map((e) => ({ ...e }));
  const r = runEntries.map((e) => ({ ...e }));
  const bc = annotateConnections(b);
  const rc = annotateConnections(r);
  const out = [];
  for (const mode of [...new Set(bc.map((c) => c.mode))]) {
    const bConn = bc.filter((c) => c.mode === mode).at(-1);
    const rConn = rc.filter((c) => c.mode === mode).at(-1);
    const bk = connectionKeys(b, bConn.index);
    const rk = rConn ? connectionKeys(r, rConn.index) : [];
    const ops = diffSequences(bk, rk);
    out.push({ mode, baselineLines: [bConn.firstLine, bConn.lastLine], runLines: rConn ? [rConn.firstLine, rConn.lastLine] : null, ops, same: ops.every((o) => o.op === '=') });
  }
  const extra = [...new Set(rc.map((c) => c.mode))].filter((m) => !bc.some((c) => c.mode === m));
  return { modes: out, runOnlyModes: extra };
}

export function formatModeDiff(cmp) {
  const lines = [];
  for (const m of cmp.modes) {
    lines.push(`== ${m.mode}: baseline lines ${m.baselineLines.join('-')}, this run ${m.runLines ? `lines ${m.runLines.join('-')}` : 'no such connection'}${m.same ? ' (same method sequence)' : ''}`);
    for (const o of m.ops) lines.push(`${o.op === '=' ? ' ' : o.op} ${o.key}`);
  }
  if (cmp.runOnlyModes.length) lines.push(`== connections only in this run: ${cmp.runOnlyModes.join(', ')}`);
  lines.push('(ambient notifications -- remoteControl/status/changed, account/*, thread/tokenUsage/updated -- are left out)');
  return lines.join('\n');
}
