#!/usr/bin/env node
// OAC Session Channels conformance runner (task E8, #48): the reference checker for the
// fixtures under tests/protocol/.
//
//   node tests/protocol/runner/run.mjs              # run every fixture and the index checks
//   node tests/protocol/runner/run.mjs --verbose    # also print each passing fixture
//   node tests/protocol/runner/run.mjs --self-test  # known-answer tests of the runner itself
//   node tests/protocol/runner/run.mjs <path>...    # run only these fixture files
//
// It reads each fixture, dispatches on `fixture_format` and then on `stage`, evaluates the
// stage from the rules of the spec documents (spec/session-channels.md §3.3, §6.10, §7.5,
// §8.5; spec/security.md §3.3; spec/bindings/mcp.md §12.2), and compares the outcome with
// the fixture's `expected`. It also checks each fixture's own form, that every error code
// a fixture expects is in Table 8.3 of spec/session-channels.md ([SC-RCP-074]), and the
// requirement indexes against the fixtures (index-check.mjs).
//
// Dependency-free: Node.js built-ins only. It runs no harness, no transport and no network.
// Exit status: 0 when every check passes, 1 otherwise.

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse, toPlain, deepEqual, DUPLICATE, JNum } from './json.mjs';
import { readTable83 } from './core.mjs';
import * as sc from './stages-sc.mjs';
import * as sec from './stages-sec.mjs';
import { mcpBinding } from './mcpb.mjs';
import { checkIndexes, DOC_OF_PREFIX, dirOfId } from './index-check.mjs';
import { selfTest } from './self-test.mjs';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..', '..');
const FIXTURES = path.join(ROOT, 'tests', 'protocol');

const CORE = 'oac-conformance-fixture/1';
const MCPB = 'oac-mcpb-fixture/1';

// Stage table: evaluator, required `expected` members, optional ones. A member listed as
// `a|b` is required in one of the alternatives the stage function's result decides; the
// runner only demands that `result`-like discriminators are present and compares every
// member the fixture states.
const STAGES = {
  [CORE]: {
    // spec/session-channels.md
    envelope: [sc.envelope, ['result'], ['error', 'trusted_security']],
    negotiation: [sc.negotiation, ['result'], ['extension', 'major']],
    binding: [sc.binding, ['result', 'attachments', 'record'], []],
    send: [sc.send, ['result'], ['from', 'version', 'error']],
    presence: [sc.presence, ['discarded', 'states'], ['send']],
    discovery: [sc.discovery, ['result'], ['sessions', 'error']],
    receipt: [sc.receipt, ['result'], ['effective_state']],
    reply: [sc.reply, ['reply_headers', 'correlation'], []],
    correlation: [sc.correlation, ['result'], ['error', 'correlation', 'answers']],
    combine: [sc.combine, ['state', 'retry_allowed'], []],
    routing: [sc.routing, ['result'], ['error']],
    receive: [sc.receive, ['result'], ['error']],
    // spec/security.md
    security: [sec.security, ['result'], ['error', 'canonical', 'receipt_permitted', 'bindings_after', 'record']],
    replay: [sec.replay, ['results'], []],
    'key-id': [sec.keyId, ['key_id'], []],
    'key-removal': [sec.keyRemoval, ['bindings_after', 'grants_after'], []],
    registration: [sec.registration, ['result'], ['canonical', 'binding_usable']],
    'receipt-auth': [sec.receiptAuth, ['result'], ['canonical']],
    'presence-auth': [sec.presenceAuth, ['result', 'record'], ['bindings_after', 'canonical', 'effective_lifetime_ms']],
    'discovery-auth': [sec.discoveryAuth, ['discoverable'], []],
    exchange: [sec.exchange, ['results'], []],
    provenance: [sec.provenance, ['result'], ['state', 'error']],
    body: [sec.body, ['quoted_lines'], []],
  },
  // spec/bindings/mcp.md §12.2
  [MCPB]: {
    'mcp-binding': [mcpBinding, ['result'], []],
  },
};

// Spec text with CR LF line ends (a Windows checkout) read as LF.
const read = (rel) => fs.readFileSync(path.join(ROOT, rel), 'utf8').replace(/\r\n/g, '\n');

function listFixtures() {
  const out = [];
  for (const dir of fs.readdirSync(FIXTURES).sort()) {
    const full = path.join(FIXTURES, dir);
    if (!fs.statSync(full).isDirectory() || !/^(sc|sec|mcpb)-[a-z]+$/.test(dir)) continue;
    for (const file of fs.readdirSync(full).sort()) out.push({ dir, file, full: path.join(full, file) });
  }
  return out;
}

// The fixture's own form (§3.3; spec/bindings/mcp.md §12.2).
function formProblems(f, fx) {
  const p = [];
  if (!f.file.endsWith('.json')) return ['not a .json file'];
  const m = /^((?:SC|SEC|MCPB)-[A-Z]+-[0-9]{3})\.([pn])([0-9]{2})-[a-z0-9-]+\.json$/.exec(f.file);
  if (!m) p.push('file name is not <REQUIREMENT-ID>.<p|n><NN>-<slug>.json');
  if (!STAGES[fx.fixture_format]) p.push(`unknown fixture_format ${JSON.stringify(fx.fixture_format)}`);
  if (m && fx.requirement !== m[1]) p.push(`requirement ${fx.requirement} does not match the file name`);
  if (m && fx.kind !== (m[2] === 'p' ? 'positive' : 'negative')) p.push(`kind ${fx.kind} does not match the file name`);
  if (fx.requirement && dirOfId(fx.requirement) !== f.dir) p.push(`requirement ${fx.requirement} does not belong in ${f.dir}/`);
  if (fx.requirement && DOC_OF_PREFIX[fx.requirement.split('-')[0]] !== fx.spec) p.push(`spec ${fx.spec} is not the document of ${fx.requirement}`);
  if (fx.spec_revision !== '0.1') p.push(`spec_revision ${fx.spec_revision} is not 0.1`);
  if ((fx.kind === 'negative') !== (typeof fx.failure_mode === 'string')) p.push('failure_mode must be present exactly on a negative fixture');
  if (typeof fx.description !== 'string' || fx.description.length === 0) p.push('description missing');
  for (const m2 of ['context', 'input', 'expected']) if (fx[m2] === null || typeof fx[m2] !== 'object') p.push(`${m2} missing`);
  return p;
}

// Every error code a fixture expects ([SC-RCP-074]): `error` members anywhere in `expected`.
function expectedCodes(v, out = []) {
  if (Array.isArray(v)) v.forEach((x) => expectedCodes(x, out));
  else if (v && typeof v === 'object') {
    for (const [k, x] of Object.entries(v)) {
      if (k === 'error' && typeof x === 'string') out.push(x);
      else expectedCodes(x, out);
    }
  }
  return out;
}

function hasDuplicate(v) {
  if (Array.isArray(v)) return v.some(hasDuplicate);
  if (v && typeof v === 'object' && !(v instanceof JNum)) return !!v[DUPLICATE] || Object.keys(v).some((k) => hasDuplicate(v[k]));
  return false;
}

function runFixture(f, env) {
  const text = fs.readFileSync(f.full, 'utf8');
  let raw;
  try {
    raw = parse(text);
  } catch (e) {
    return { problems: [`not JSON: ${e.message}`] };
  }
  const fx = { ...raw, requirement: raw.requirement, expected: toPlain(raw.expected) };
  const problems = formProblems(f, toPlain(raw));
  if (hasDuplicate(raw)) problems.push('a member name repeats in the fixture file');
  for (const code of expectedCodes(fx.expected)) {
    if (!env.table83.has(code)) problems.push(`expected error code ${code} is not in Table 8.3 ([SC-RCP-074])`);
  }
  const stages = STAGES[raw.fixture_format];
  const stage = stages && stages[raw.stage];
  if (!stage) {
    problems.push(`stage ${JSON.stringify(raw.stage)} is not defined for ${raw.fixture_format}`);
    return { problems, fixture: toPlain(raw) };
  }
  const [evaluate, required, optional] = stage;
  for (const k of required) if (!Object.prototype.hasOwnProperty.call(fx.expected, k)) problems.push(`expected.${k} missing`);
  for (const k of Object.keys(fx.expected)) if (!required.includes(k) && !optional.includes(k)) problems.push(`expected.${k} is not a member of stage ${raw.stage}`);
  let actual;
  try {
    actual = evaluate(fx, env);
  } catch (e) {
    problems.push(`runner error: ${e.message}`);
    return { problems, fixture: toPlain(raw) };
  }
  const mismatches = [];
  for (const k of Object.keys(fx.expected)) {
    let want = fx.expected[k];
    // A discovery result lists its sessions in any order (§7.5).
    if (raw.stage === 'discovery' && k === 'sessions' && Array.isArray(want)) want = [...want].sort();
    if (!deepEqual(actual[k], want)) mismatches.push({ member: k, expected: fx.expected[k], actual: actual[k] });
  }
  return { problems, mismatches, fixture: toPlain(raw), stage: raw.stage };
}

function main() {
  const args = process.argv.slice(2);
  if (args.includes('--self-test')) {
    const failures = selfTest();
    for (const f of failures) console.log(`FAIL self-test: ${f}`);
    console.log(failures.length ? `Result: FAIL (${failures.length} self-test failures)` : 'Result: PASS (self-test)');
    process.exit(failures.length ? 1 : 0);
  }
  const verbose = args.includes('--verbose');
  const only = args.filter((a) => !a.startsWith('--')).map((a) => path.resolve(a));
  const env = { table83: readTable83(read('spec/session-channels.md')) };
  const all = listFixtures();
  const selected = only.length ? all.filter((f) => only.includes(path.resolve(f.full))) : all;
  let failed = 0;
  const tally = new Map();
  const parsed = [];
  for (const f of selected) {
    const r = runFixture(f, env);
    parsed.push({ dir: f.dir, file: f.file, fixture: r.fixture });
    const key = `${r.fixture ? r.fixture.fixture_format : '?'} ${r.stage || '?'}`;
    const t = tally.get(key) || { pass: 0, fail: 0 };
    const ok = r.problems.length === 0 && (r.mismatches || []).length === 0;
    if (ok) t.pass++;
    else t.fail++;
    tally.set(key, t);
    const name = `${f.dir}/${f.file}`;
    if (ok) {
      if (verbose) console.log(`PASS ${name}`);
      continue;
    }
    failed++;
    console.log(`FAIL ${name}`);
    for (const p of r.problems) console.log(`  - ${p}`);
    for (const m of r.mismatches || []) console.log(`  - expected.${m.member}: expected ${JSON.stringify(m.expected)}, got ${JSON.stringify(m.actual)}`);
  }
  let indexProblems = [];
  if (!only.length) {
    indexProblems = checkIndexes(read, parsed);
    for (const p of indexProblems) console.log(`FAIL index: ${p}`);
  }
  console.log('');
  for (const [k, t] of [...tally.entries()].sort()) console.log(`${k.padEnd(48)} ${String(t.pass).padStart(4)} pass ${t.fail ? `${t.fail} FAIL` : ''}`);
  const total = selected.length;
  console.log(`\nFixtures: ${total - failed}/${total} pass. Index checks: ${only.length ? 'skipped' : indexProblems.length ? `${indexProblems.length} problems` : 'clean'}.`);
  const bad = failed > 0 || indexProblems.length > 0;
  console.log(bad ? 'Result: FAIL' : 'Result: PASS');
  process.exit(bad ? 1 : 0);
}

main();
