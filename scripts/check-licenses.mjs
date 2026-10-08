#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
//
// License headers and the dependency license inventory (#50, F1; C1 section 10,
// docs/planning/v0.1/07-repository-and-dependencies.md sections 5-7, oac-release section 2).
//
//   node scripts/check-licenses.mjs              # headers + inventory for this workspace
//   node scripts/check-licenses.mjs --self-test  # planted violations must fail
//   node scripts/check-licenses.mjs --mutation-test
//       # copy the real workspace to a temp dir, plant a GPL dependency (plain, and optional
//       # behind a non-default feature) in its Cargo.toml, run real cargo: each must fail
//
// 1. Headers. Every git-tracked Rust source (*.rs) and every git-tracked Cargo.toml outside
//    tools/ carries `SPDX-License-Identifier: Apache-2.0` in its first five lines. The
//    copyright-holder line and NOTICE are I2's job (oac-release section 2 item 6), not this
//    script's.
// 2. Workspace crates. Every workspace member declares `license = "Apache-2.0"`.
// 3. Inventory. Every package in the resolved graph (`cargo metadata --all-features`, all
//    targets and platforms, so optional dependencies are included) is listed with name,
//    version, source and license. A third-party package passes when its SPDX expression
//    has at least one OR-arm made only of accepted licenses (07 section 5): the permissive
//    list, and the weak, file-level copyleft licenses OAC takes as unmodified dependencies
//    (lead clarification 2026-10-08, docs/planning/decisions/G-7-stage4-dependencies.md
//    section 3). The listing records the elected arm: Apache-2.0 whenever the expression
//    offers it (07 section 6, C1 sections 7 and 10), otherwise a permissive arm, otherwise
//    an arm with a weak-copyleft term, which the listing flags. A copyleft identifier in a
//    non-elected arm is flagged too, never passed silently. Strong copyleft (the GPL family,
//    AGPL, SSPL, OSL and the like) is never accepted. No license expression (only
//    license-file), an unknown identifier, or no accepted arm, fails: that dependency
//    needs a recorded decision before it lands.
//
// Limits, stated plainly: the inventory reads the license field each crate declares; it
// does not read license files or scan source. A git dependency would be listed from its own
// manifest (the Codex crates that once were candidates are refused, G-7 section 2). The full transitive audit and
// NOTICE remain I2 (Stage 6). Node built-ins only; no cargo plugin; `cargo metadata` runs
// with --offline (the shared METADATA_ARGS of scripts/check-crate-deps.mjs).
// Exit codes: 0 = clean; 1 = violation; 2 = usage or environment error.

import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { METADATA_ARGS, addDep, addOptionalDep, cargoMetadata, withWorkspaceCopy } from './check-crate-deps.mjs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptPath = fileURLToPath(import.meta.url);
const repoRoot = resolve(dirname(scriptPath), '..');

const WORKSPACE_LICENSE = 'Apache-2.0';
const HEADER = /SPDX-License-Identifier:\s*Apache-2\.0\b/;
const HEADER_LINES = 5;

// The accepted licenses: exactly the lists recorded in
// docs/planning/v0.1/07-repository-and-dependencies.md section 5, "Accepted licenses".
// Adding one is a license-policy change: record it there first, in the same PR.
// Unicode-3.0: unicode-ident, #51; BSD-3-Clause: ed25519-dalek and its curve25519-dalek,
// x25519-dalek and subtle, #52 (C5 section 2). Both under the operator decision
// https://github.com/RossGraeber/OAC/issues/51#issuecomment-6009697192 (07 section 5).
// Zlib, ISC, BSD-2-Clause and CDLA-Permissive-2.0: the zenoh, rustls, ring and rcgen graph
// (#7, G-7 section 3), lead decision in chat 2026-10-08 under the same permissive policy.
// With ISC here, ring's "Apache-2.0 AND ISC" is an accepted arm as written.
const PERMISSIVE = new Set([
  'Apache-2.0',
  'MIT',
  '0BSD',
  'Unicode-3.0',
  'BSD-3-Clause',
  'Zlib',
  'ISC',
  'BSD-2-Clause',
  'CDLA-Permissive-2.0',
]);
// Weak, file-level copyleft, accepted for an unmodified dependency: it cannot make OAC
// (Apache-2.0) relicense, which is what the no-copyleft rule guards against (lead
// clarification in chat 2026-10-08, G-7 section 3). Elected only when no permissive arm is
// offered, and flagged in the listing. A statically linked LGPL crate may carry relink
// obligations; I3 (#79) lists them in the Stage 6 inventory.
const WEAK_COPYLEFT = new Set([
  'MPL-2.0',
  'LGPL-2.1-only',
  'LGPL-2.1-or-later',
  'LGPL-3.0-only',
  'LGPL-3.0-or-later',
  'EPL-2.0',
]);
const ACCEPTED = new Set([...PERMISSIVE, ...WEAK_COPYLEFT]);
// Copyleft families, flagged wherever they appear (oac-release section 2 item 3). The strong
// ones (GPL, AGPL, SSPL, OSL, ...) are on neither list above, so an arm holding one fails.
const COPYLEFT = /^(?:A?GPL|LGPL|MPL|EPL|EUPL|CDDL|OSL|CPL|CECILL|CC-BY-SA|SSPL)\b/i;

// Split an SPDX expression into OR-arms, each a list of AND-ed license terms. A recursive
// descent parser: nested parentheses, AND binding tighter than OR, `WITH`, and the legacy
// `/` separator. Returns null if unparseable.
export function spdxArms(expr) {
  if (typeof expr !== 'string' || expr.trim() === '') return null;
  const tokens = expr.replace(/\//g, ' OR ').replace(/([()])/g, ' $1 ').trim().split(/\s+/);
  let pos = 0;
  // Grammar: or := and (OR and)*; and := term (AND term)*; term := '(' or ')' | id [WITH id]
  const parseOr = () => {
    let arms = parseAnd();
    while (tokens[pos] === 'OR') {
      pos++;
      arms = [...arms, ...parseAnd()];
    }
    return arms;
  };
  const parseAnd = () => {
    let arms = parseTerm();
    while (tokens[pos] === 'AND') {
      pos++;
      const right = parseTerm();
      arms = arms.flatMap((a) => right.map((b) => [...a, ...b]));
    }
    return arms;
  };
  const parseTerm = () => {
    const t = tokens[pos++];
    if (t === undefined || t === ')' || t === 'OR' || t === 'AND' || t === 'WITH') throw new Error('bad SPDX');
    if (t === '(') {
      const arms = parseOr();
      if (tokens[pos++] !== ')') throw new Error('unbalanced');
      return arms;
    }
    if (tokens[pos] === 'WITH') {
      pos++;
      const ex = tokens[pos++];
      if (!ex || ex === '(' || ex === ')') throw new Error('bad WITH');
      return [[`${t} WITH ${ex}`]];
    }
    return [[t]];
  };
  try {
    const arms = parseOr();
    return pos === tokens.length ? arms : null;
  } catch {
    return null;
  }
}

// Verdict for one third-party package: { ok, elected, flags, weak, reason }. `weak` lists
// the weak-copyleft terms of the elected arm (empty when a permissive arm is elected).
export function licenseVerdict(license) {
  const arms = spdxArms(license);
  if (!arms) return { ok: false, flags: [], weak: [], reason: license ? `unparseable license expression` : 'no license expression (license-file only or none)' };
  const flags = [...new Set(arms.flat().filter((t) => COPYLEFT.test(t)))];
  // OAC elects Apache-2.0 whenever a dual offers it (07 section 6, C1 section 10);
  // otherwise a permissive arm that includes Apache-2.0 (as in (MIT OR Apache-2.0) AND
  // Unicode-3.0); otherwise the first permissive arm; otherwise the first arm whose
  // copyleft terms are all weak (07 section 5, G-7 section 3).
  const permissive = (arm) => arm.every((t) => PERMISSIVE.has(t));
  const accepted = (arm) => arm.every((t) => ACCEPTED.has(t));
  const elected =
    arms.find((arm) => arm.length === 1 && arm[0] === 'Apache-2.0') ??
    arms.find((arm) => permissive(arm) && arm.includes('Apache-2.0')) ??
    arms.find(permissive) ??
    arms.find(accepted);
  if (!elected) return { ok: false, flags, weak: [], reason: 'no OR-arm made only of accepted licenses (07 section 5)' };
  return { ok: true, elected: elected.join(' AND '), flags, weak: elected.filter((t) => WEAK_COPYLEFT.has(t)) };
}

export function checkHeaders(files, read) {
  const v = [];
  for (const f of files) {
    const head = read(f).split(/\r?\n/).slice(0, HEADER_LINES).join('\n');
    if (!HEADER.test(head)) v.push(`${f}: no "SPDX-License-Identifier: Apache-2.0" in its first ${HEADER_LINES} lines`);
  }
  return v;
}

export function checkInventory(meta) {
  const violations = [];
  const rows = [];
  const members = new Set(meta.workspace_members);
  const reached = meta.resolve ? new Set(meta.resolve.nodes.map((n) => n.id)) : new Set(meta.packages.map((p) => p.id));
  for (const p of meta.packages) {
    if (!reached.has(p.id)) continue;
    const source = members.has(p.id) ? 'workspace' : p.source?.startsWith('git+') ? 'git' : p.source ? 'registry' : 'path';
    let verdict;
    if (members.has(p.id)) {
      const ok = p.license === WORKSPACE_LICENSE;
      verdict = ok ? 'ok' : 'FAIL';
      if (!ok) violations.push(`${p.name}: workspace crate license is ${JSON.stringify(p.license)}, expected "${WORKSPACE_LICENSE}"`);
    } else {
      const r = licenseVerdict(p.license);
      verdict = r.ok ? `ok (elects ${r.elected})` : 'FAIL';
      if (r.ok && r.weak.length) verdict += `; weak copyleft elected, unmodified dependency: ${r.weak.join(', ')}`;
      else if (r.flags.length) verdict += `; copyleft arm flagged: ${r.flags.join(', ')}`;
      if (!r.ok) violations.push(`${p.name} ${p.version}: ${r.reason} (license: ${JSON.stringify(p.license ?? null)})`);
    }
    rows.push({ name: p.name, version: p.version, source, license: p.license ?? '(none)', verdict });
  }
  rows.sort((a, b) => (a.source === 'workspace') - (b.source === 'workspace') || a.name.localeCompare(b.name));
  return { violations, rows };
}

function printInventory(rows) {
  console.log('| Crate | Version | Source | License | Verdict |');
  console.log('|---|---|---|---|---|');
  for (const r of rows) console.log(`| ${r.name} | ${r.version} | ${r.source} | ${r.license} | ${r.verdict} |`);
  const third = rows.filter((r) => r.source !== 'workspace').length;
  console.log(`\n${rows.length} package(s): ${rows.length - third} workspace, ${third} third-party.`);
}

function gitTracked() {
  const r = spawnSync('git', ['ls-files', '-z', '--', '*.rs', '*Cargo.toml'], { cwd: repoRoot, encoding: 'utf8' });
  if (r.status !== 0) throw new Error(`git ls-files failed: ${r.stderr}`);
  return r.stdout.split('\0').filter((f) => f && !f.startsWith('tools/') && (f.endsWith('.rs') || /(^|\/)Cargo\.toml$/.test(f)));
}

function run() {
  const files = gitTracked();
  const headerV = checkHeaders(files, (f) => readFileSync(join(repoRoot, f), 'utf8'));
  const m = cargoMetadata(repoRoot, ['--locked']);
  if (m.status !== 0) {
    console.error(m.stderr);
    return 2;
  }
  const { violations, rows } = checkInventory(JSON.parse(m.stdout));
  printInventory(rows);
  const all = [...headerV, ...violations];
  console.log(`headers: ${files.length} tracked file(s) checked, ${headerV.length} missing`);
  for (const v of all) console.log(`FAIL  ${v}`);
  console.log(all.length ? `licenses: FAIL -- ${all.length} violation(s)` : 'licenses: CLEAN');
  return all.length ? 1 : 0;
}

// ---------------------------------------------------------------------------------------

function selfTest() {
  const cases = [];
  const expect = (name, cond) => cases.push({ name, ok: !!cond });
  const ok = (l) => licenseVerdict(l).ok;
  expect('control: Apache-2.0 passes', ok('Apache-2.0'));
  expect('control: MIT OR Apache-2.0 passes', ok('MIT OR Apache-2.0'));
  expect('control: legacy MIT/Apache-2.0 passes', ok('MIT/Apache-2.0'));
  expect('control: (MIT OR Apache-2.0) AND 0BSD passes', ok('(MIT OR Apache-2.0) AND 0BSD'));
  expect('control: nested ((MIT OR Apache-2.0) AND (0BSD OR GPL-2.0-only)) passes',
    ok('((MIT OR Apache-2.0) AND (0BSD OR GPL-2.0-only))'));
  // N2: Apache-2.0 is elected whenever a dual offers it (07 section 6, C1 section 10).
  expect('election: MIT OR Apache-2.0 elects Apache-2.0', licenseVerdict('MIT OR Apache-2.0').elected === 'Apache-2.0');
  expect('election: 0BSD OR Apache-2.0 elects Apache-2.0', licenseVerdict('0BSD OR Apache-2.0').elected === 'Apache-2.0');
  expect('election: MIT/Apache-2.0 elects Apache-2.0', licenseVerdict('MIT/Apache-2.0').elected === 'Apache-2.0');
  expect('election: MIT alone elects MIT', licenseVerdict('MIT').elected === 'MIT');
  // N3: only the licenses 07 section 5 records are accepted.
  // #51: Unicode-3.0 is on the 07 section 5 list (unicode-ident), electing the Apache arm.
  expect('control: (MIT OR Apache-2.0) AND Unicode-3.0 passes, electing Apache-2.0 AND Unicode-3.0',
    licenseVerdict('(MIT OR Apache-2.0) AND Unicode-3.0').elected === 'Apache-2.0 AND Unicode-3.0');
  expect('Unicode-DFS-2016 fails (not in 07 section 5)', !ok('Unicode-DFS-2016'));
  // #52: BSD-3-Clause is on the 07 section 5 list (ed25519-dalek, C5 section 2); a single
  // license with no OR-arm, so it is elected as is. BSD-2-Clause is not on the list.
  expect('control: BSD-3-Clause passes, electing BSD-3-Clause', licenseVerdict('BSD-3-Clause').elected === 'BSD-3-Clause');
  // #7 (G-7 section 3): Zlib, ISC, BSD-2-Clause and CDLA-Permissive-2.0 are permissive and
  // on the list; ring's "Apache-2.0 AND ISC" passes as written.
  expect('control: BSD-2-Clause passes', licenseVerdict('BSD-2-Clause').elected === 'BSD-2-Clause');
  expect('control: Zlib passes', licenseVerdict('Zlib').elected === 'Zlib');
  expect('control: ISC passes', licenseVerdict('ISC').elected === 'ISC');
  expect('control: CDLA-Permissive-2.0 passes', licenseVerdict('CDLA-Permissive-2.0').elected === 'CDLA-Permissive-2.0');
  expect('control: Apache-2.0 AND ISC (ring) passes, electing Apache-2.0 AND ISC',
    licenseVerdict('Apache-2.0 AND ISC').elected === 'Apache-2.0 AND ISC');
  expect('BSD-1-Clause fails (not in 07 section 5)', !ok('BSD-1-Clause'));
  expect('ISC AND OpenSSL fails (OpenSSL not in 07 section 5)', !ok('ISC AND OpenSSL'));
  // Lead clarification 2026-10-08 (G-7 section 3): weak, file-level copyleft passes as an
  // unmodified dependency and is reported; strong copyleft fails.
  const mpl = licenseVerdict('MPL-2.0');
  expect('control: MPL-2.0 passes, weak copyleft reported', mpl.ok && mpl.weak.includes('MPL-2.0'));
  for (const l of ['LGPL-2.1-only', 'LGPL-2.1-or-later', 'LGPL-3.0-only', 'LGPL-3.0-or-later', 'EPL-2.0']) {
    expect(`control: ${l} passes`, ok(l));
  }
  expect('control: MIT AND LGPL-2.1-only passes', ok('MIT AND LGPL-2.1-only'));
  expect('election: MPL-2.0 OR MIT elects MIT (permissive arm first)', licenseVerdict('MPL-2.0 OR MIT').elected === 'MIT');
  expect('election: MIT OR Apache-2.0 OR LGPL-2.1-or-later (r-efi) elects Apache-2.0',
    licenseVerdict('MIT OR Apache-2.0 OR LGPL-2.1-or-later').elected === 'Apache-2.0');
  expect('election: EPL-2.0 OR Apache-2.0 elects Apache-2.0 (zenoh)', licenseVerdict('EPL-2.0 OR Apache-2.0').elected === 'Apache-2.0');
  for (const l of ['GPL-3.0-only', 'GPL-3.0-or-later', 'GPL-2.0-only', 'GPL-2.0-or-later', 'AGPL-3.0-only', 'AGPL-3.0-or-later', 'SSPL-1.0', 'OSL-3.0', 'GPL-3.0']) {
    expect(`${l} fails (strong copyleft)`, !ok(l));
  }
  expect('control: MIT OR GPL-3.0 passes on its permissive arm', licenseVerdict('MIT OR GPL-3.0').elected === 'MIT');
  expect('MIT AND GPL-3.0 fails', !ok('MIT AND GPL-3.0'));
  expect('MPL-2.0 AND GPL-2.0-or-later fails', !ok('MPL-2.0 AND GPL-2.0-or-later'));
  expect('LGPL-2.1+ (deprecated spelling) fails as an unknown identifier', !ok('LGPL-2.1+'));
  expect('BSD-3-Clause AND GPL-2.0-only fails', !ok('BSD-3-Clause AND GPL-2.0-only'));
  expect('Apache-2.0 WITH LLVM-exception fails (not in 07 section 5)', !ok('Apache-2.0 WITH LLVM-exception'));
  // B1: optional dependencies are in the graph the inventory reads.
  expect('cargo metadata runs with --all-features', METADATA_ARGS.includes('--all-features'));
  const z = licenseVerdict('EPL-2.0 OR Apache-2.0');
  expect('control: EPL-2.0 OR Apache-2.0 passes, electing Apache-2.0, copyleft flagged',
    z.ok && z.elected === 'Apache-2.0' && z.flags.includes('EPL-2.0'));
  expect('GPL-3.0-only fails', !ok('GPL-3.0-only'));
  expect('missing license (license-file only) fails', !ok(null));
  expect('empty license fails', !ok(''));
  expect('unknown identifier fails', !ok('Proprietary'));
  expect('unbalanced parentheses fail', !ok('(MIT OR Apache-2.0'));
  expect('dangling OR fails', !ok('MIT OR'));
  expect('WITH on a base outside the accepted list fails', !ok('GPL-2.0-only WITH Classpath-exception-2.0'));

  const files = { 'core/src/lib.rs': '// SPDX-License-Identifier: Apache-2.0\n', 'x/src/lib.rs': '//! doc\n', 'late.rs': '\n\n\n\n\n// SPDX-License-Identifier: Apache-2.0\n', 'mit.rs': '// SPDX-License-Identifier: MIT\n' };
  const hv = checkHeaders(Object.keys(files), (f) => files[f]);
  expect('header: present passes, missing / too late / wrong license each fail', hv.length === 3 && !hv.some((v) => v.startsWith('core/')));

  const meta = (pkgs) => ({
    workspace_members: pkgs.filter((p) => p.ws).map((p) => p.id),
    packages: pkgs,
    resolve: { nodes: pkgs.map((p) => ({ id: p.id, deps: [] })) },
  });
  const ws = { id: 'w', name: 'oac-core', version: '0.0.0', license: 'Apache-2.0', source: null, ws: true };
  expect('control: inventory of the scaffold passes', checkInventory(meta([ws])).violations.length === 0);
  expect('workspace crate under MIT fails', checkInventory(meta([{ ...ws, license: 'MIT' }])).violations.length === 1);
  expect('workspace crate with no license fails', checkInventory(meta([{ ...ws, license: null }])).violations.length === 1);
  const mplRow = checkInventory(meta([ws, { id: 'oe', name: 'option-ext', version: '0.2.0', license: 'MPL-2.0', source: 'registry+x' }]));
  expect('control: an MPL-2.0 crate (option-ext) passes and its listing reports the weak copyleft',
    mplRow.violations.length === 0 && mplRow.rows.some((r) => /weak copyleft elected, unmodified dependency: MPL-2\.0/.test(r.verdict)));
  expect('third-party GPL crate fails', checkInventory(meta([ws, { id: 'g', name: 'g', version: '1.0.0', license: 'GPL-3.0-only', source: 'registry+https://github.com/rust-lang/crates.io-index' }])).violations.length === 1);
  const zrow = checkInventory(meta([ws, { id: 'z', name: 'zenoh', version: '1.10.1', license: 'EPL-2.0 OR Apache-2.0', source: 'registry+x' }]));
  expect('control: dual EPL/Apache crate passes and is flagged in the listing',
    zrow.violations.length === 0 && zrow.rows.some((r) => /elects Apache-2\.0\); copyleft arm flagged: EPL-2\.0/.test(r.verdict)));
  // B1: a GPL crate reached only through an optional, feature-gated edge. Under
  // --all-features cargo resolves that edge, so the crate is a node of the graph.
  const optGpl = meta([{ ...ws, id: 'w2' }, { id: 'o', name: 'gpl-stub', version: '0.0.1', license: 'GPL-3.0-only', source: null }]);
  optGpl.resolve.nodes[0].deps = [{ name: 'stub', pkg: 'o', dep_kinds: [{ kind: null, target: null }] }];
  expect('optional GPL dependency (feature-gated edge) fails', checkInventory(optGpl).violations.length === 1);

  let failed = 0;
  for (const c of cases) {
    console.log(`${c.ok ? 'pass' : 'FAIL'}  ${c.name}`);
    if (!c.ok) failed++;
  }
  console.log(`self-test: ${cases.length - failed}/${cases.length} cases pass`);
  return failed ? 1 : 0;
}

// ---------------------------------------------------------------------------------------
// --mutation-test: plant GPL dependencies in a copy of the real workspace, run real cargo.

const LICENSE_MUTATIONS = [
  { name: 'core/ depends on a GPL-3.0-only crate', file: 'core/Cargo.toml', edit: addDep('dependencies', 'gpl-stub = { path = "../../stubs/gpl-stub" }') },
  {
    name: 'adapters/claude optionally depends on a GPL-3.0-only crate (feature "leak")',
    file: 'adapters/claude/Cargo.toml',
    edit: addOptionalDep('stub = { package = "gpl-stub", path = "../../../stubs/gpl-stub", optional = true }'),
  },
  {
    // A uniquely named stub (#7): adding a `zenoh` key would collide with the real zenoh
    // dependency transports/zenoh takes in G1 (#62). Same proof: a dual with an EPL-2.0 arm
    // passes, electing Apache-2.0.
    name: 'control: transports/zenoh depends on an EPL-2.0 OR Apache-2.0 crate (elects Apache-2.0)',
    control: true,
    file: 'transports/zenoh/Cargo.toml',
    edit: addDep('dependencies', 'epl-dual-stub = { path = "../../../stubs/epl-dual-stub" }'),
  },
];

function mutationTest() {
  const runInventory = (dir) => {
    const m = cargoMetadata(dir);
    if (m.status !== 0) return { error: (m.stderr.split(/\r?\n/).find((l) => /error/i.test(l)) ?? 'cargo metadata failed').trim() };
    return checkInventory(JSON.parse(m.stdout));
  };
  let bad = 0;
  const cases = [{ name: 'control: unmodified copy of the workspace', control: true }, ...LICENSE_MUTATIONS];
  for (const c of cases) {
    const r = withWorkspaceCopy(c.edit ? c : null, runInventory);
    let ok;
    let detail;
    if (r.error) {
      ok = false;
      detail = `cargo error (malformed mutation?): ${r.error}`;
    } else {
      ok = c.control ? r.violations.length === 0 : r.violations.length > 0;
      detail = r.violations[0] ?? `${r.rows.length} package(s) listed, 0 violations`;
    }
    if (!ok) bad++;
    console.log(`${ok ? 'pass' : 'FAIL'}  ${c.name} -- ${detail}`);
  }
  console.log(`mutation test: ${cases.length - bad}/${cases.length} cases pass`);
  return bad ? 1 : 0;
}

function main(argv) {
  if (argv[0] === '--self-test') return selfTest();
  if (argv[0] === '--mutation-test') return mutationTest();
  if (argv.length) {
    console.error('usage: check-licenses.mjs [--self-test | --mutation-test]');
    return 2;
  }
  return run();
}

if (process.argv[1] && resolve(process.argv[1]) === scriptPath) {
  try {
    process.exitCode = main(process.argv.slice(2));
  } catch (e) {
    console.error(e?.stack ?? String(e));
    process.exitCode = 2;
  }
}
