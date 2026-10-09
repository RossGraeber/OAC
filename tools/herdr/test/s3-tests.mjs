// Unit checks for tools/herdr/scenarios/s3-codex-capture.mjs (#343 review, finding 5): the
// send-once guard, the idle-add precondition stop and the cases-preinit version filter
// (c1de8f4). Each check fails when its guard is reverted: the helper's own behaviour is
// checked, and so is the scenario's use of it at the place it guards. They run on every OS
// (run.mjs --self-test, unit half); the scenario has no lifecycle case against the fakes.

import { readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { DriverError } from '../lib/herdr.mjs';
import { g2Facts, parseG2Transcript } from '../lib/g2.mjs';
import {
  COMMITTED_CLIENT, COMMITTED_CLIENT_SHA256, CASE_SETS, S3_GATE, S3_NON_VERDICT, onceGuard, idleAddRefusal, wireVersionsOf, s3FixtureNames, s3UnverifiedNames,
} from '../scenarios/s3-codex-capture.mjs';
import { stageClientCopy } from '../lib/g2.mjs';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO = resolve(HERE, '..', '..', '..');
const read = (p) => readFileSync(p, 'utf8').replace(/\r\n/g, '\n');
const SCENARIO = read(join(REPO, 'tools', 'herdr', 'scenarios', 's3-codex-capture.mjs'));
const S3_FIXTURE = 'docs/planning/gates/fixtures/s3-codex-capture/transcript-2026-10-07-0.161.0-herdr.jsonl';
const throws = (fn, cls, re) => {
  try {
    fn();
    return false;
  } catch (e) {
    return (!cls || e instanceof cls) && (!re || re.test(e.message));
  }
};
// The source text from `start` to the first `end` after it, or '' when either is missing.
const between = (start, end) => {
  const i = SCENARIO.indexOf(start);
  const j = i < 0 ? -1 : SCENARIO.indexOf(end, i);
  return i < 0 || j < 0 ? '' : SCENARIO.slice(i, j);
};

export function s3Unit(check) {
  // --- the send-once guard --------------------------------------------------------------------
  const log = [];
  const once = onceGuard(log);
  once('a');
  once('b');
  check('s3 once: each send is logged once; a second send of the same thing throws a DriverError and is not logged', throws(() => once('a'), DriverError, /a second time; nothing is re-sent/) && log.map((x) => x.what).join() === 'a,b');
  check('s3 once: the scenario builds its guard from onceGuard, and guards the idle add and the cases before each runs', /const once = onceGuard\(s3\.injectionsSent\);/.test(SCENARIO) &&
    /once\('thread\/queue\/add to the idle thread \(client `idleadd`\)'\);\n\s*const ia = await runClient\('idleadd'/.test(SCENARIO) &&
    /once\('the cases \(client `cases`\)'\);\n(?:.*\n){0,2}\s*const cr = await runClient\('cases'/.test(SCENARIO));

  // --- the idle-add precondition stop -------------------------------------------------------------
  const turns = (status) => ({ line: 7, turns: status ? [{ id: 't', status }] : [] });
  check('s3 precondition: only a last turn `completed` lets the idle add run', idleAddRefusal(turns('completed')) === null &&
    ['interrupted', 'failed', 'inProgress'].every((s) => new RegExp(`last turn is \`${s}\`.*nothing sent`).test(idleAddRefusal(turns(s)) ?? '')) &&
    /not on record/.test(idleAddRefusal(turns(null)) ?? '') && /not on record/.test(idleAddRefusal(null) ?? ''));
  const sixA = between('// --- 6a. the idle add', "runClient('idleadd'");
  check('s3 precondition: the scenario stops on a refusal before the idle add is sent or guarded', /const refusal = idleAddRefusal\(priorTurns\);\n\s*if \(refusal\) stop\(refusal\);\n\s*once\(/.test(sixA), sixA.slice(-400));

  // --- c1de8f4: cases-preinit carries no version --------------------------------------------------
  const conns = g2Facts(parseG2Transcript(read(join(REPO, S3_FIXTURE)))).connections;
  const unfiltered = [...new Set(conns.map((c) => c.userAgentVersion))];
  check('s3 versions (c1de8f4): on the recorded S3 transcript, every connection but cases-preinit reports 0.161.0; wireVersionsOf leaves preinit out', unfiltered.includes(null) && conns.some((c) => c.mode === 'cases-preinit' && c.userAgentVersion === null) && JSON.stringify(wireVersionsOf(conns)) === '["0.161.0"]', JSON.stringify(unfiltered));
  check('s3 versions (c1de8f4): a connection other than cases-preinit with no version still counts', JSON.stringify(wireVersionsOf([{ mode: 'cases-main', userAgentVersion: null }, { mode: 'list', userAgentVersion: '1.0.0' }])) === '[null,"1.0.0"]');
  check('s3 versions (c1de8f4): the post-run check uses wireVersionsOf', /const wireVersions = wireVersionsOf\(facts\(\)\.connections\);/.test(SCENARIO));

  // --- names, labels, the staged client -------------------------------------------------------------
  check('s3 names: the full set keeps G2\'s fixture names; another set adds its name before -herdr; unverified names stay unverified-*', JSON.stringify(s3FixtureNames('2026-10-07', '0.161.0')) === '{"transcript":"transcript-2026-10-07-0.161.0-herdr.jsonl","pane":"pane-2026-10-07-0.161.0-herdr.txt"}' &&
    s3FixtureNames('2026-10-08', '0.161.0', 'queued-interrupt').transcript === 'transcript-2026-10-08-0.161.0-queued-interrupt-herdr.jsonl' && s3UnverifiedNames('2026-10-08', 'queued-interrupt').pane === 'unverified-pane-2026-10-08-queued-interrupt-herdr.txt' && JSON.stringify(CASE_SETS) === '["all","queued-interrupt"]');
  check('s3 labels (review finding 4): its own gate label on every version finding and its own nonVerdictBearing text; no G2 text', S3_GATE === 'S3 capture' && !/G2|K7/.test(S3_NON_VERDICT) && /nonVerdictBearing: S3_NON_VERDICT/.test(SCENARIO) &&
    (SCENARIO.match(/codexVersionWarning\(\{/g) ?? []).length === (SCENARIO.match(/gate: S3_GATE \}\)\)/g) ?? []).length && /pinsReadWarning\(pin, S3_GATE\)/.test(SCENARIO) && !/'G2'\)/.test(SCENARIO));
  const tmp = mkdtempSync(join(tmpdir(), 'oac-s3-unit-'));
  try {
    const st = stageClientCopy(REPO, COMMITTED_CLIENT, tmp);
    check('s3: the capture client is staged from the blob at HEAD, sha256 = the one the scenario was written for', st.match && st.committedSha256 === COMMITTED_CLIENT_SHA256 && st.workingTreeMatchesHead === true);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
}
