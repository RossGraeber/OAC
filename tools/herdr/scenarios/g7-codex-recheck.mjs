// g7-codex-recheck: the G7 follow-up live check of risk rows 66 and 73 on the current Codex
// (#470; docs/planning/v0.1/11-risks.md), driven through herdr. NOT A GATE RUN and NOT
// VERDICT-BEARING: it records what a real Codex app-server does, on the machine it runs on. It
// never changes a gate verdict, STATUS.md's gate table, PINS.md or the spec.
//
// There is no TUI and no operator message. The driver runs one quarantined client
// (docs/planning/gates/fixtures/g7-codex-recheck/client.mjs.throwaway-quarantined, staged from
// its blob at HEAD, sha256 checked) against the OPERATOR'S ALREADY RUNNING shared daemon's control
// socket (through `codex app-server proxy`), on threads the client starts itself in a scratch
// directory of its own. It never touches a thread it did not start; a frame about any other
// thread is not recorded (a stub is). The driver runs only `codex --version` (the manifest does)
// and `codex app-server daemon version` (read-only). It never runs the daemon's start, stop or
// restart verbs: with no running daemon the run is NOT RUN.
//
// Operator command (herdr at the PINS.md pin, the shared daemon already running, a signed-in
// Codex CLI of any version: versions float, a difference from PINS.md is a VERSION WARNING
// finding, never a stop, #216):
//
//   node tools/herdr/run.mjs --scenario g7-codex-recheck --out <run dir>
//
// Parameters: --param quietMs=25000 (the "nothing happens" window), --param cases=all|row66|row73.
//
// What the client sends, and what the scenario judges (judgeG7, below; every verdict rests on
// the observations the client wrote into the transcript, each citing a request and its answer):
//
//   Row 66 (a), first half: on a thread of its own, a busy turn; two thread/queue/add during it;
//     turn/interrupt; a quiet window; then a turn/start of its own. Judged: both adds accepted,
//     nothing dispatched in the quiet window, and the two items run, one per idle, in order, after
//     that next uninterrupted turn.
//   Row 66 (a), second half: on another thread, a completed turn, an add with an extra member
//     (case d, below), a busy turn, turn/interrupt, then a thread/queue/add to the idle thread whose
//     last turn was interrupted, a quiet window, a turn/start of its own. Judged: the add is
//     accepted, nothing dispatches in the quiet window, and the item runs after the next turn.
//   Row 66 (b): a busy turn, three adds, turn/interrupt (so the items are held), then a SECOND
//     daemon client, one that never started or resumed the thread, sends thread/queue/list,
//     update (item 2's text), reorder (3, 2, 1), delete (item 1) and list; then an uninterrupted
//     turn/start and the observation of what dispatches. Judged per operation: accepted, visible
//     in the list, and visible in dispatch (item 3, then item 2 as updated, item 1 never).
//     ATTEMPTED because the schema of this Codex defines the methods (`codex app-server
//     generate-json-schema --experimental`: ThreadQueueListParams, ThreadQueueUpdateParams,
//     ThreadQueueReorderParams, ThreadQueueDeleteParams; thread/queue/list|update|delete|reorder
//     among the ClientRequest methods, checked 2026-10-31 on codex-cli 0.165.0). The oac-codex-
//     appserver skill and the spec's schema tables do not list them yet; they are experimental.
//   Row 66 (c): on the thread of case a2, thread/unsubscribe, a wait for thread/closed, a check
//     with thread/loaded/list that it is not loaded, a thread/queue/add, a quiet window, a check that
//     it is still not loaded, then thread/resume from a new connection. Judged: the add is accepted,
//     nothing happens and the thread stays unloaded until the resume, and the resume dispatches it.
//     If the thread did not unload within the bound the case is INCONCLUSIVE.
//   Row 66 (d): on an idle thread whose last turn completed, a thread/queue/add with one member
//     beyond threadId, clientUserMessageId and input. Judged: accepted, and the message runs
//     with exactly the text sent (the member had no visible effect).
//   Row 73: on a thread of its own, loaded and idle after ONE minimal turn (a resume needs the
//     thread's stored history, which exists from the first user message), a thread/resume from a
//     second connection carrying `approvalPolicy` (below), then the same on a thread that is not
//     loaded (the control), each followed by a thread/resume without it. Recorded: the response's
//     approvalPolicy, any thread/settings/updated notification in a quiet window, the readback.
//     Judged: APPLIED-AND-RETAINED, REPORTED-NOT-RETAINED, NOT-APPLIED or REFUSED for the loaded
//     thread, and whether the control shows the readback to be live state. No turn is started
//     after the override, so what the app-server does with it in a turn is not observed.
//
// Why `approvalPolicy` for row 73. spec/bindings/mcp.md [MCPB-CDX-006] names six members that a
// subscribing thread/resume MUST NOT carry (`model`, `cwd`, `approvalPolicy`, `sandbox`,
// `permissions`, `config`; adapters/codex/tests/schema.rs L183-L191 lists the same six). Of them
// `approvalPolicy` is harmless and observable: ThreadResumeParams has it (a string enum:
// untrusted, on-request, never, or a granular object), and ThreadResumeResponse reports
// `approvalPolicy` (required) as does the thread/settings/updated notification
// (ThreadSettings.approvalPolicy), so the resume answer, a later resume and the notification can
// all show it, with no turn. The client sets `untrusted` (the thread asks before it runs anything
// not trusted), or `on-request` if the thread already has `untrusted`, so it differs from the
// thread's own value. `model` could name a model the account does not have; `cwd` and `sandbox`
// change what a later turn may touch; `config` is free-form.
//
// This client DELIBERATELY DOES WHAT OAC ITSELF MUST NEVER DO ([MCPB-CDX-006]): it is a test of
// the app-server, not a use of the adapter, and nothing in it is product code.
//
// Hard limits (enforced in the client, checked by test/g7-tests.mjs): it never sends turn/steer
// (the client sends only the methods on its ALLOWED_METHODS list); it never answers an approval
// or an elicitation request (it records it and stops the case); it reads no credential file,
// database or stored-history file and never logs in; it archives only the threads it started
// (S3 did the same). Nothing here reads a file under the Codex home or asks an OS credential
// store for anything.
//
// Captures: `transcript-g7recheck-<date>-<version>-herdr.jsonl` (the client's own wire
// transcript, sanitized as G2's; `-<set>-` before `-herdr` when --param cases is not `all`), or
// `unverified-transcript-g7recheck-<date>-herdr.jsonl` when the CLI, the daemon and the wire
// do not report one and the same version. The observations and judgments are also in the run
// manifest (`g7`). No pane is captured: there is no pane.

import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { NotRunError, DriverError } from '../lib/herdr.mjs';
import { parseCodexVersions, pinsReadWarning, parseCodexCliVersion, parseCodexDaemonVersion, codexVersionWarning, CODEX_PIN_ROW, CODEX_DAEMON_VERSION_FIELDS } from '../lib/pins.mjs';
import { harnessVersions } from '../lib/manifest.mjs';
import { runBounded } from '../lib/proc.mjs';
import { CODEX_DAEMON_SCRATCH_HOLDER } from '../lib/scratch.mjs';
import { committedFile } from '../lib/g1.mjs';
import { PINS_PATH, stageClientCopy, parseG2Transcript, g2Facts, sanitizeTranscript } from '../lib/g2.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..', '..');

// The quarantined client, and the sha256 of the blob this scenario was written for.
export const COMMITTED_CLIENT = 'docs/planning/gates/fixtures/g7-codex-recheck/client.mjs.throwaway-quarantined';
export const COMMITTED_CLIENT_SHA256 = '3ab5c817e3a361f57df9884e970ddc0ddad6112276faf5bec88b37c667ea876a';
export const G7_GATE = 'G7 recheck';
export const G7_NON_VERDICT = '#470: a live re-check of risk rows 66 and 73 for G7 (#68); compared against no other run, not a gate run, never changes a gate verdict';
export const CASE_SETS = Object.freeze(['all', 'row66', 'row73']);
export const CLIENT_MODE = 'g7-observation';

const cap = (s, n) => (String(s ?? '').length > n ? `${String(s).slice(0, n)}… (${String(s).length} chars)` : String(s ?? ''));

// --- capture names ---------------------------------------------------------------------------

const withSet = (name, set) => (set === 'all' ? name : name.replace(/-herdr\.(\w+)$/, `-${set}-herdr.$1`));
export function g7Names(date, version, set = 'all') {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(date)) throw new Error(`bad date ${JSON.stringify(date)}`);
  const v = /^\d+\.\d+\.\d+$/.test(version ?? '') ? version : null;
  return { transcript: withSet(v ? `transcript-g7recheck-${date}-${v}-herdr.jsonl` : `unverified-transcript-g7recheck-${date}-herdr.jsonl`, set) };
}

// The Codex versions the wire reported, one per connection that sent initialize.
export const wireVersionsOf = (connections) => [...new Set(connections.map((c) => c.userAgentVersion))];

// --- observations and judgments --------------------------------------------------------------

// The client's observations from its transcript entries (lib/g2.mjs parseG2Transcript): a Map
// name -> observed (the last wins), plus the list in order.
export function readObservations(entries) {
  const list = entries.filter((e) => e.direction === 'observation' && e.payload && typeof e.payload.name === 'string').map((e) => ({ line: e.line, name: e.payload.name, observed: e.payload.observed }));
  return { list, map: new Map(list.map((o) => [o.name, o.observed])) };
}
// The transcript without the observation entries: what lib/g2.mjs reads as connections.
export const wireEntries = (entries) => entries.filter((e) => e.direction !== 'observation');

const V = Object.freeze({ CONFIRMED: 'CONFIRMED', CONTRADICTED: 'CONTRADICTED', INCONCLUSIVE: 'INCONCLUSIVE' });
const stoppedOr = (map, need) => {
  for (const n of need) if (!map.has(n)) return `no observation \`${n}\`${map.has('case-stopped') ? ` (a case stopped: ${JSON.stringify(map.get('case-stopped')?.reason ?? null)})` : ''}`;
  return null;
};
const j = (id, claim, verdict, basis, detail = {}) => ({ id, claim, verdict, basis, ...detail });

// Row 66 and row 73 judged from the observation map. Pure; unit-tested.
export function judgeG7(map, { quietMs = null } = {}) {
  const g = (n) => map.get(n);
  const row66 = [];
  const holds = (id, claim, needs, fn) => {
    const missing = stoppedOr(map, needs);
    row66.push(missing ? j(id, claim, V.INCONCLUSIVE, missing) : fn());
  };

  holds('66a-queued-at-interrupt', '(a) items already queued when a turn is interrupted wait, and run after the next uninterrupted turn', ['a1-add-1', 'a1-add-2', 'a1-interrupt-completed', 'a1-dispatch-within-quiet-window', 'a1-first-queued-turn', 'a1-second-queued-turn', 'a1-queued-turns'], () => {
    const c = 'items queued before the interrupt wait and run after the next uninterrupted turn';
    if (!g('a1-add-1').accepted || !g('a1-add-2').accepted) return j('66a-queued-at-interrupt', c, V.INCONCLUSIVE, `an add was refused: ${JSON.stringify([g('a1-add-1').error, g('a1-add-2').error])}`);
    if (g('a1-interrupt-completed').status !== 'interrupted') return j('66a-queued-at-interrupt', c, V.INCONCLUSIVE, `the turn ended \`${g('a1-interrupt-completed').status}\`, not interrupted; the case did not set up its precondition`);
    if (g('a1-dispatch-within-quiet-window').dispatched) return j('66a-queued-at-interrupt', c, V.CONTRADICTED, `a turn started within the ${g('a1-dispatch-within-quiet-window').quietMs} ms window after the interrupt, with no uninterrupted turn in between`);
    const f = g('a1-first-queued-turn');
    const s = g('a1-second-queued-turn');
    const t = g('a1-queued-turns');
    if (!f.ran || !s.ran) return j('66a-queued-at-interrupt', c, V.CONTRADICTED, `nothing started in the quiet window, but after the next uninterrupted turn ${!f.ran && !s.ran ? 'neither queued item' : !f.ran ? 'the first queued item' : 'the second queued item'} did not run within the bound`);
    const ok = f.afterNextTurn === true && s.afterNextTurn === true && t.distinct === true && t.inOrder === true && t.onePerIdle === true;
    return j('66a-queued-at-interrupt', c, ok ? V.CONFIRMED : V.CONTRADICTED, ok ? 'both adds accepted; nothing dispatched in the quiet window; after the next uninterrupted turn the two items ran in separate turns, in order, the second after the first completed' : `the items ran, but not as recorded at 0.161.0: ${JSON.stringify({ firstAfterNext: f.afterNextTurn, secondAfterNext: s.afterNextTurn, ...t })}`);
  });

  holds('66a-add-after-interrupt', '(a) an add made after turn/interrupt to the idle thread waits, and runs after the next uninterrupted turn', ['a2-add', 'a2-interrupt-completed', 'a2-dispatch-within-quiet-window', 'a2-queued-turn'], () => {
    const c = 'an add to an idle thread whose last turn was interrupted waits until a turn completes uninterrupted';
    if (!g('a2-add').accepted) return j('66a-add-after-interrupt', c, V.CONTRADICTED, `the add was refused: ${JSON.stringify(g('a2-add').error)}`);
    if (g('a2-interrupt-completed').status !== 'interrupted') return j('66a-add-after-interrupt', c, V.INCONCLUSIVE, `the turn ended \`${g('a2-interrupt-completed').status}\`, not interrupted`);
    if (g('a2-dispatch-within-quiet-window').dispatched) return j('66a-add-after-interrupt', c, V.CONTRADICTED, `a turn started within the ${g('a2-dispatch-within-quiet-window').quietMs} ms window after the add`);
    const q = g('a2-queued-turn');
    if (!q.ran) return j('66a-add-after-interrupt', c, V.CONTRADICTED, 'the add waited, but did not run after the next uninterrupted turn within the bound');
    return j('66a-add-after-interrupt', c, q.afterNextTurn === true ? V.CONFIRMED : V.CONTRADICTED, q.afterNextTurn === true ? 'accepted; nothing in the quiet window; ran after the next turn completed' : 'it ran, but not after the next turn completed');
  });

  holds('66c-unloaded-thread', '(c) an add to an unloaded thread waits; thread/resume dispatches it', ['c-thread-closed', 'c-loaded-before-add', 'c-add', 'c-events-within-quiet-window', 'c-loaded-after-add', 'c-resume', 'c-queued-turn'], () => {
    const c = 'an add to an unloaded thread is accepted and waits; loading the thread dispatches it';
    if (!g('c-thread-closed').closed || g('c-loaded-before-add').loaded !== false) return j('66c-unloaded-thread', c, V.INCONCLUSIVE, `the thread was not unloaded when the add was made (thread/closed seen: ${g('c-thread-closed').closed}; still in thread/loaded/list: ${g('c-loaded-before-add').loaded})`);
    if (!g('c-add').accepted) return j('66c-unloaded-thread', c, V.CONTRADICTED, `the add was refused: ${JSON.stringify(g('c-add').error)}`);
    if (g('c-events-within-quiet-window').method !== null || g('c-loaded-after-add').loaded !== false) return j('66c-unloaded-thread', c, V.CONTRADICTED, `the add loaded or ran the thread without a resume (event ${JSON.stringify(g('c-events-within-quiet-window').method)}, loaded after add ${g('c-loaded-after-add').loaded})`);
    if (g('c-resume').error) return j('66c-unloaded-thread', c, V.INCONCLUSIVE, `thread/resume answered with an error: ${JSON.stringify(g('c-resume').error)}`);
    const q = g('c-queued-turn');
    return j('66c-unloaded-thread', c, q.ran ? V.CONFIRMED : V.CONTRADICTED, q.ran ? 'accepted; no event and still unloaded through the quiet window; thread/resume then dispatched it' : 'the resume did not dispatch the queued item within the bound');
  });

  holds('66d-extra-member', '(d) an extra member in a thread/queue/add request is accepted and ignored', ['d-add'], () => {
    const c = 'an extra member in thread/queue/add is accepted and ignored';
    if (!g('d-add').accepted) return j('66d-extra-member', c, V.CONTRADICTED, `the request was refused: ${JSON.stringify(g('d-add').error)}`);
    const t = g('d-turn');
    if (!t?.ran) return j('66d-extra-member', c, V.INCONCLUSIVE, 'accepted, but the message did not run within the bound on this idle thread, so its effect is not observed');
    const ok = t.textMatches === true && t.completed === 'completed';
    return j('66d-extra-member', c, ok ? V.CONFIRMED : V.CONTRADICTED, ok ? 'accepted; the message ran as one turn with exactly the text sent' : `accepted, but the turn's user text ${t.textMatches ? 'matched' : 'differs from what was sent'} and it ended \`${t.completed}\``);
  });

  // (b)
  {
    const id = '66b-queue-management';
    const claim = '(b) other daemon clients can reorder, update or delete a queued item';
    const needs = ['b-adds', 'b-interrupt-completed', 'b-list-before', 'b-update', 'b-reorder', 'b-delete', 'b-list-after', 'b-held-during-management', 'b-dispatched'];
    const missing = stoppedOr(map, needs);
    if (missing) row66.push(j(id, claim, V.INCONCLUSIVE, missing));
    else {
      const adds = g('b-adds');
      const before = g('b-list-before');
      const after = g('b-list-after');
      const disp = g('b-dispatched');
      const [t1, t2, t3] = adds.texts;
      const updated = disp.expectedTexts[1];
      const afterTexts = after.items ? after.items.map((i) => i.text) : null;
      const beforeTexts = before.items ? before.items.map((i) => i.text) : null;
      const ops = {
        update: { accepted: !g('b-update').error, error: g('b-update').error, inList: afterTexts ? afterTexts.includes(updated) : null, ran: disp.texts.includes(updated) },
        reorder: { accepted: !g('b-reorder').error, error: g('b-reorder').error, inList: afterTexts ? afterTexts.indexOf(t3) >= 0 && afterTexts.indexOf(t3) < afterTexts.indexOf(updated) : null, ran: disp.texts.indexOf(t3) >= 0 && disp.texts.indexOf(t3) < disp.texts.indexOf(updated) },
        delete: { accepted: !g('b-delete').error && g('b-delete').deleted === true, error: g('b-delete').error, deleted: g('b-delete').deleted, inList: afterTexts ? !afterTexts.includes(t1) : null, ran: !disp.deletedItemRan },
      };
      const detail = { operations: ops };
      if (!adds.accepted.every(Boolean)) row66.push(j(id, claim, V.INCONCLUSIVE, 'an add was refused, so there was no queue to manage', detail));
      else if (g('b-interrupt-completed').status !== 'interrupted' || g('b-held-during-management').turnStartedMeanwhile) row66.push(j(id, claim, V.INCONCLUSIVE, 'the items were not held while they were managed (the turn was not interrupted, or a turn started meanwhile), so a change cannot be told from a dispatch', detail));
      else if (Object.values(ops).every((o) => !o.accepted)) row66.push(j(id, claim, V.CONTRADICTED, `thread/queue/update, reorder and delete from a client that did not start the thread were all refused (update: ${JSON.stringify(ops.update.error)}); thread/queue/list answered ${before.error ? JSON.stringify(before.error) : 'normally'}`, detail));
      else if (before.error || !beforeTexts || JSON.stringify(beforeTexts) !== JSON.stringify([t1, t2, t3])) row66.push(j(id, claim, V.INCONCLUSIVE, `thread/queue/list before the changes did not show the three items in order (${before.error ? JSON.stringify(before.error) : JSON.stringify(beforeTexts)})`, detail));
      else {
        const refused = Object.entries(ops).filter(([, o]) => !o.accepted).map(([k]) => k);
        const effective = Object.values(ops).every((o) => o.inList === true && o.ran === true);
        if (refused.length) row66.push(j(id, claim, V.CONTRADICTED, `thread/queue/${refused.join(', ')} from a client that did not start the thread was refused or not applied`, detail));
        else if (!effective) row66.push(j(id, claim, V.CONTRADICTED, 'the three calls were accepted, but their effect is missing from the list or from dispatch', detail));
        else row66.push(j(id, claim, V.CONFIRMED, 'update, reorder and delete from a second client were accepted, shown by thread/queue/list, and dispatch followed them (item 3, then item 2 as updated; item 1 never ran)', detail));
      }
    }
  }

  // Row 73
  let row73;
  {
    const id = '73-resume-override';
    const claim = 'whether the app-server applies thread/resume setting overrides to a thread that is already loaded';
    const needs = ['override-chosen', 'l-loaded', 'l-resume-baseline', 'l-resume-override', 'l-settings-updated-notification', 'l-resume-readback'];
    const missing = stoppedOr(map, needs);
    if (missing) row73 = j(id, claim, V.INCONCLUSIVE, missing, { outcome: 'INCONCLUSIVE' });
    else {
      const O = g('override-chosen').value;
      const same = (a) => JSON.stringify(a ?? null) === JSON.stringify(O);
      const base = g('l-resume-baseline').settings?.approvalPolicy;
      const r1 = g('l-resume-override');
      const r2 = g('l-resume-readback');
      const note = g('l-settings-updated-notification');
      const control = { present: map.has('u-resume-override') && map.has('u-resume-readback'), unloaded: g('u-thread-closed')?.closed === true && g('u-loaded-before-resume')?.loaded === false, applied: null, retained: null };
      if (control.present && control.unloaded) {
        control.applied = !g('u-resume-override').error && same(g('u-resume-override').settings?.approvalPolicy);
        control.retained = !g('u-resume-readback').error && same(g('u-resume-readback').settings?.approvalPolicy);
      }
      const facts = {
        override: { member: 'approvalPolicy', value: O },
        threadHad: g('override-chosen').threadHas,
        baselineResume: base ?? null,
        overrideResponse: { error: r1.error, approvalPolicy: r1.settings?.approvalPolicy ?? null, matchesOverride: same(r1.settings?.approvalPolicy) },
        notification: note.seen,
        readback: { error: r2.error, approvalPolicy: r2.settings?.approvalPolicy ?? null, matchesOverride: same(r2.settings?.approvalPolicy) },
        control,
      };
      const caveats = ['judged from the wire only: no turn was started after the override, so its effect on a turn is not observed'];
      if (control.present && control.unloaded && control.retained === false) caveats.push('the control thread (not loaded) did not show the override in a later resume, so a later resume may not read live settings');
      if (!control.present || !control.unloaded) caveats.push('the control did not run to its end (thread not unloaded, or a case stopped), so whether a later resume reads live settings is not shown');
      let outcome;
      let basis;
      if (!g('l-loaded').loaded) {
        outcome = 'INCONCLUSIVE';
        basis = 'the thread was not in thread/loaded/list when the override was sent';
      } else if (r1.error) {
        outcome = 'REFUSED';
        basis = `the app-server answered the override with an error: ${JSON.stringify(r1.error)}`;
      } else if (!same(r1.settings?.approvalPolicy)) {
        outcome = 'NOT-APPLIED';
        basis = `the response reports ${JSON.stringify(r1.settings?.approvalPolicy ?? null)}, not the override ${JSON.stringify(O)}; the thread's own value is kept`;
      } else if (same(r2.settings?.approvalPolicy)) {
        outcome = 'APPLIED-AND-RETAINED';
        basis = `the response reports the override and a later resume without it reports it too${note.seen.length ? `; thread/settings/updated was sent (${note.seen.length} connection(s))` : '; no thread/settings/updated was sent'}`;
      } else {
        outcome = 'REPORTED-NOT-RETAINED';
        basis = `the response reports the override, but a later resume without it reports ${JSON.stringify(r2.settings?.approvalPolicy ?? null)}`;
      }
      // For row 73 `verdict` is the observation's standing, not a claim about the spec: `outcome` is the answer.
      row73 = j(id, claim, outcome === 'INCONCLUSIVE' ? V.INCONCLUSIVE : 'OBSERVED', basis, { outcome, facts, caveats });
    }
  }
  return { row66, row73, quietMs };
}

// --- the scenario ----------------------------------------------------------------------------

export default {
  name: 'g7-codex-recheck',
  description: 'G7 follow-up (#470): live re-check of risk rows 66 and 73 on the current Codex, by a quarantined client against the running shared daemon. No TUI. Not a gate run; not verdict-bearing.',
  harnesses: ['codex'],
  defaults: {
    launch: ['codex'], // recorded only: the driver launches nothing
    timeboxMs: 60 * 60 * 1000,
    params: {
      cases: 'all', // CASE_SETS
      quietMs: '25000', // the client's "nothing happens" window
      casesTimeoutMs: '1500000',
      cliTimeoutMs: '60000',
      clientTimeoutMs: '60000',
    },
  },

  async run(ctx) {
    const { params } = ctx;
    const num = (k) => {
      const v = Number(params[k]);
      if (!(v >= 0)) throw new DriverError(`--param ${k} must be a non-negative number`);
      return v;
    };
    const caseSet = params.cases || 'all';
    if (!CASE_SETS.includes(caseSet)) throw new DriverError(`--param cases must be one of ${CASE_SETS.join(', ')}`);
    if (!/^\d+$/.test(String(params.quietMs))) throw new DriverError('--param quietMs must be a whole number of ms');
    const abortSignal = ctx.herdr.abortSignal;

    const g7 = {
      nonVerdictBearing: G7_NON_VERDICT,
      params: { ...params },
      date: new Date().toISOString().slice(0, 10),
      versions: null,
      client: null,
      daemon: { versionBefore: null, versionAfter: null },
      clientRuns: [],
      captureNames: null,
      observations: null,
      judgments: null,
      stoppedAt: null,
    };
    let clientDir = null;

    const stop = (reason) => {
      ctx.herdr.inputHalted ??= reason;
      g7.stoppedAt = reason;
      throw new NotRunError(reason);
    };
    const aborted = () => {
      if (abortSignal?.aborted) throw new NotRunError(ctx.herdr.abortReason());
    };
    const say = (text) => console.error(`\n[g7-codex-recheck] ${text}\n`);

    // The Codex CLI. Only `app-server daemon version` is ever run here, as the verb after it.
    const codexCli = async (args, label) => {
      aborted();
      const [file, argv] = process.platform === 'win32' ? ['cmd.exe', ['/d', '/s', '/c', 'codex', ...args]] : ['codex', args];
      const r = await runBounded(file, argv, { deadlineMs: Math.min(num('cliTimeoutMs'), Math.max(1, ctx.remainingMs())), env: process.env, abortSignal });
      aborted();
      if (r.timedOut) stop(`\`codex ${args.join(' ')}\` timed out after ${num('cliTimeoutMs')} ms; nothing re-run`);
      return { r, rec: { label, argv: ['codex', ...args], startedAt: r.startedAt, endedAt: r.endedAt, exitCode: r.exitCode, timedOut: r.timedOut, spawnError: r.spawnError } };
    };

    const transcriptPath = () => join(clientDir, 'transcript.jsonl');
    const allEntries = () => (clientDir && existsSync(transcriptPath()) ? parseG2Transcript(readFileSync(transcriptPath(), 'utf8'), { completeLinesOnly: true }) : []);
    const facts = () => g2Facts(wireEntries(allEntries()));
    const lineCount = () => allEntries().at(-1)?.line ?? 0;

    const runClient = async (mode, args, { deadlineMs = num('clientTimeoutMs') } = {}) => {
      aborted();
      const linesBefore = lineCount();
      const r = await runBounded(process.execPath, [join(clientDir, 'client.mjs'), mode, ...args], { deadlineMs: Math.min(deadlineMs, Math.max(1, ctx.remainingMs())), env: process.env, cwd: clientDir, abortSignal });
      aborted();
      const rec = {
        mode,
        args,
        startedAt: r.startedAt,
        endedAt: r.endedAt,
        exitCode: r.exitCode,
        signal: r.signal ?? null,
        timedOut: r.timedOut,
        stdoutStatus: String(r.stdout ?? '').split('\n').filter((l) => /^\[(?:done|error|upgrade failed|server-request ignored[^\]]*)\]/.test(l)).map((l) => cap(l.trimEnd(), 600)),
        stderr: cap(String(r.stderr ?? '').trim(), 2000),
        linesBefore,
        linesAfter: lineCount(),
      };
      g7.clientRuns.push(rec);
      if (r.timedOut) stop(`the client's \`${mode}\` run timed out after ${deadlineMs} ms; nothing re-sent`);
      if (r.spawnError) throw new DriverError(`could not run the staged client (${r.spawnError})`);
      const mine = facts().connections.filter((c) => c.firstLine > linesBefore);
      const problems = [];
      if (!mine.length) problems.push('no connection on the wire (no WebSocket upgrade recorded)');
      for (const c of mine) {
        if (c.upgraded === false) problems.push(`WebSocket upgrade refused: ${c.handshake}`);
        if (!c.userAgent) problems.push('initialize returned no userAgent');
        if (c.unparsed) problems.push(`${c.unparsed} unparsed frame(s)`);
      }
      if (r.exitCode !== 0) problems.push(`client exited ${r.exitCode ?? r.signal ?? 'abnormally'}`);
      if (mode === 'cases' && !rec.stdoutStatus.some((l) => l.startsWith('[done]'))) problems.push('the client printed no `[done]` line');
      rec.problems = problems;
      if (problems.length) {
        const msg = `the committed G7 client did not work unmodified on this Codex version: \`${mode}\`: ${problems.join('; ')}. Recorded; the client is not patched to get past it`;
        ctx.finding(msg);
        throw new DriverError(msg);
      }
      return { rec, connections: mine };
    };

    try {
      // --- 0. preflight ------------------------------------------------------------------------
      const cliRaw = ctx.harnessVersion('codex');
      const pinsFile = committedFile(REPO, PINS_PATH);
      // Versions float and are never gated (#216): every difference from PINS.md's minimum or
      // last tested version is a VERSION WARNING finding, and the run continues.
      const pin = parseCodexVersions(pinsFile.bytes.toString('utf8'));
      const cli = parseCodexCliVersion(cliRaw);
      g7.versions = {
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
        g7.versions.warnings.push(w);
        ctx.finding(w);
      };
      if (!pinsFile.workingTreeMatchesHead) ctx.finding(`${PINS_PATH} has uncommitted changes; the version check read the committed PINS.md (HEAD ${pinsFile.headCommit})`);
      warn(pinsReadWarning(pin, G7_GATE));
      if (!cli && /^N\/A/.test(cliRaw ?? 'N/A')) stop(`codex --version could not be run (${cliRaw ?? 'not recorded'}); nothing started`);
      warn(codexVersionWarning({ observed: cli, lastTested: pin.lastTested, minimum: pin.minimum, source: '`codex --version`', gate: G7_GATE }));
      g7.captureNames = g7Names(g7.date, null, caseSet);

      clientDir = ctx.dir('g7-client');
      const staged = stageClientCopy(REPO, COMMITTED_CLIENT, clientDir);
      g7.client = {
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

      // --- 1. the shared daemon is running (read-only check; the driver never starts it) ----------
      ctx.noteScratchHolder?.(CODEX_DAEMON_SCRATCH_HOLDER);
      const dv = await codexCli(['app-server', 'daemon', 'version'], 'daemon version');
      const daemonV = parseCodexDaemonVersion(dv.r.stdout);
      g7.daemon.versionBefore = { ...dv.rec, parsed: daemonV };
      g7.versions.daemon = daemonV;
      if (!daemonV || daemonV.status !== 'running') {
        stop('the shared Codex app-server daemon is not running (`codex app-server daemon version` reports no running status); the driver never starts, stops or restarts it: the operator starts it themselves. Nothing was started');
      }
      for (const k of CODEX_DAEMON_VERSION_FIELDS) {
        warn(codexVersionWarning({ observed: daemonV?.[k] ?? null, lastTested: pin.lastTested, minimum: pin.minimum, source: `\`codex app-server daemon version\` ${k}`, gate: G7_GATE }));
      }

      // --- 2. the wire version ---------------------------------------------------------------------
      const hello = await runClient('hello', []);
      const conn = hello.connections[0];
      g7.versions.wireUserAgent = conn.userAgent;
      g7.versions.wire = conn.userAgentVersion;
      warn(codexVersionWarning({ observed: conn.userAgentVersion, lastTested: pin.lastTested, minimum: pin.minimum, source: 'the wire initialize userAgent', gate: G7_GATE }));
      const sameVersion = !!cli && conn.userAgentVersion === cli && CODEX_DAEMON_VERSION_FIELDS.every((k) => daemonV?.[k] === cli);
      if (sameVersion) {
        g7.versions.verified = true;
        g7.versions.matchesLastTested = cli === pin.lastTested;
        g7.captureNames = g7Names(g7.date, cli, caseSet);
      } else {
        ctx.finding(`the Codex CLI (${cliRaw}), daemon (${JSON.stringify(daemonV)}) and wire (${conn.userAgentVersion ?? 'none'}) do not report one and the same version; the run continues, but its capture stays unverified-* because it cannot name one Codex version`);
      }

      // --- 3. the cases: threads the client starts itself, in a scratch directory of its own ------------
      say(`Running the row 66 / row 73 cases against the running shared daemon (cases=${caseSet}, quietMs=${params.quietMs}). No TUI and no input from you are needed; this takes several minutes.`);
      const casesDir = ctx.dir('g7-cases-project');
      const cr = await runClient('cases', [casesDir, caseSet, String(num('quietMs'))], { deadlineMs: num('casesTimeoutMs') });
      const ignoredRequests = cr.rec.stdoutStatus.filter((l) => l.startsWith('[server-request ignored'));
      if (ignoredRequests.length) ctx.finding(`the daemon sent the G7 client ${ignoredRequests.length} server-to-client request(s), which it never answers; the case that was running stopped: ${ignoredRequests.join('; ')}`);

      // --- 4. observations and judgments ---------------------------------------------------------------
      const { list, map } = readObservations(allEntries());
      for (const o of list) {
        if (o.name === 'case-error') ctx.finding(`case \`${o.observed.case}\` ended with an error inside the client: ${o.observed.message}; its rows are INCONCLUSIVE`);
        if (o.name === 'case-stopped') ctx.finding(`case \`${o.observed.case}\` stopped: ${o.observed.reason}`);
      }
      g7.observations = list;
      const verdicts = judgeG7(map, { quietMs: num('quietMs') });
      g7.judgments = verdicts;
      for (const r of verdicts.row66) {
        if (caseSet === 'row73') break;
        if (r.verdict === V.CONTRADICTED) ctx.finding(`risk row 66 ${r.id} is CONTRADICTED on this Codex (${cli}): ${r.basis}. The 2026-10-07/08 record said otherwise. No spec text was changed; report this to the lead`);
      }
      say(`Judgments (not verdicts of a gate):\n${[...(caseSet === 'row73' ? [] : verdicts.row66), ...(caseSet === 'row66' ? [] : [verdicts.row73])].map((r) => `  ${r.id}: ${r.outcome ?? r.verdict} - ${r.basis}`).join('\n')}`);

      // --- 5. post-run versions --------------------------------------------------------------------------------
      const post = await harnessVersions(['codex']);
      const dv2 = await codexCli(['app-server', 'daemon', 'version'], 'daemon version (post-run)');
      g7.daemon.versionAfter = { ...dv2.rec, parsed: parseCodexDaemonVersion(dv2.r.stdout) };
      const postCli = parseCodexCliVersion(post.codex);
      // "Unchanged through the run" compares with the version the run started on, not with PINS.md: versions float (#216).
      const postDaemonOk = CODEX_DAEMON_VERSION_FIELDS.every((k) => g7.daemon.versionAfter.parsed?.[k] === cli);
      const wireVersions = wireVersionsOf(facts().connections);
      g7.postRun = { cliOutput: post.codex, cli: postCli, daemon: g7.daemon.versionAfter.parsed, wireVersionsSeen: wireVersions, matches: !!cli && postCli === cli && postDaemonOk && wireVersions.length === 1 && wireVersions[0] === cli };
      if (!g7.postRun.matches) {
        ctx.finding(`the Codex version changed during the run or differed between connections (CLI ${cliRaw} before, ${post.codex} after; daemon after ${JSON.stringify(g7.daemon.versionAfter.parsed)}; wire versions ${JSON.stringify(wireVersions)}); the daemon can update itself mid-run (PINS.md "Version policy"). The run is not stopped, but the capture loses its fixture name`);
        g7.versions.verified = false;
        g7.captureNames = g7Names(g7.date, null, caseSet);
      }
    } finally {
      // The capture is sanitized structurally (lib/g2.mjs) before run.mjs's own redaction;
      // installation ids are also registered as run literals so no other copy survives.
      let transcriptText = null;
      if (clientDir && existsSync(transcriptPath())) {
        const s = sanitizeTranscript(readFileSync(transcriptPath(), 'utf8'), { ownThreadId: null });
        for (const id of s.installationIds) ctx.redactLiteral(id, '<INSTALLATION_ID>');
        g7.sanitizer = { ownThreadId: null, ...s.report };
        transcriptText = s.text;
      }
      ctx.record('g7', g7);
      if (g7.captureNames && transcriptText !== null) ctx.capture(g7.captureNames.transcript, transcriptText, { format: 'jsonl' });
    }
  },
};
