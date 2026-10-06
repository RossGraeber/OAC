#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
//
// License headers and the dependency license inventory (#50, F1; C1 section 10,
// docs/planning/v0.1/07-repository-and-dependencies.md sections 5-7, oac-release section 2).
//
//   node scripts/check-licenses.mjs              # headers + inventory for this workspace
//   node scripts/check-licenses.mjs --self-test  # planted violations must fail
//
// 1. Headers. Every git-tracked Rust source (*.rs) and every git-tracked Cargo.toml outside
//    tools/ carries `SPDX-License-Identifier: Apache-2.0` in its first five lines. The
//    copyright-holder line and NOTICE are I2's job (oac-release section 2 item 6), not this
//    script's.
// 2. Workspace crates. Every workspace member declares `license = "Apache-2.0"`.
// 3. Inventory. Every package in the resolved graph (`cargo metadata`, all targets and
//    platforms) is listed with name, version, source and license. A third-party package
//    passes when its SPDX expression has at least one OR-arm made only of licenses in the
//    PERMISSIVE allowlist below (that arm is the one OAC elects, as C1 section 7 does for
//    zenoh). A copyleft identifier in a non-elected arm is flagged in the listing, never
//    passed silently. No license expression (only license-file), or no permissive arm,
//    fails: that dependency needs a recorded decision before it lands.
//
// Limits, stated plainly: the inventory reads the license field each crate declares; it
// does not read license files or scan source. Git dependencies (the Codex app-server
// crates, 07 section 8) are listed from their own manifests. The full transitive audit and
// NOTICE remain I2 (Stage 6). Node built-ins only; no cargo plugin; `cargo metadata` runs
// with --offline. Exit codes: 0 = clean; 1 = violation; 2 = usage or environment error.

import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptPath = fileURLToPath(import.meta.url);
const repoRoot = resolve(dirname(scriptPath), '..');

const WORKSPACE_LICENSE = 'Apache-2.0';
const HEADER = /SPDX-License-Identifier:\s*Apache-2\.0\b/;
const HEADER_LINES = 5;

// Permissive licenses compatible with distributing the oac binary under Apache-2.0.
// Adding to this list is a license-policy change: record it in 07 section 5 first.
const PERMISSIVE = new Set([
  'Apache-2.0',
  'Apache-2.0 WITH LLVM-exception',
  'MIT',
  'MIT-0',
  'BSD-2-Clause',
  'BSD-3-Clause',
  '0BSD',
  'ISC',
  'Zlib',
  'Unicode-3.0',
  'Unicode-DFS-2016',
  'CC0-1.0',
  'BSL-1.0',
  'Unlicense',
]);
// Copyleft families, flagged wherever they appear (oac-release section 2 item 3).
const COPYLEFT = /^(?:A?GPL|LGPL|MPL|EPL|EUPL|CDDL|OSL|CPL|CECILL|CC-BY-SA|SSPL)\b/i;

// Split an SPDX expression into OR-arms, each a list of AND-ed license terms. Handles
// parentheses one level deep and the legacy `/` separator. Returns null if unparseable.
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

// Verdict for one third-party package: { ok, elected, flags, reason }.
export function licenseVerdict(license) {
  const arms = spdxArms(license);
  if (!arms) return { ok: false, flags: [], reason: license ? `unparseable license expression` : 'no license expression (license-file only or none)' };
  const flags = [...new Set(arms.flat().filter((t) => COPYLEFT.test(t)))];
  const elected = arms.find((arm) => arm.every((t) => PERMISSIVE.has(t)));
  if (!elected) return { ok: false, flags, reason: 'no OR-arm made only of permissive licenses' };
  return { ok: true, elected: elected.join(' AND '), flags };
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
      if (r.flags.length) verdict += `; copyleft arm flagged: ${r.flags.join(', ')}`;
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
  const m = spawnSync('cargo', ['metadata', '--format-version', '1', '--offline', '--locked'], {
    cwd: repoRoot,
    encoding: 'utf8',
    maxBuffer: 256 * 1024 * 1024,
  });
  if (m.error || m.status !== 0) {
    console.error(m.error ? String(m.error) : m.stderr);
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
  expect('control: (MIT OR Apache-2.0) AND Unicode-3.0 passes', ok('(MIT OR Apache-2.0) AND Unicode-3.0'));
  expect('control: Apache-2.0 WITH LLVM-exception passes', ok('Apache-2.0 WITH LLVM-exception'));
  const z = licenseVerdict('EPL-2.0 OR Apache-2.0');
  expect('control: EPL-2.0 OR Apache-2.0 passes, electing Apache-2.0, copyleft flagged',
    z.ok && z.elected === 'Apache-2.0' && z.flags.includes('EPL-2.0'));
  expect('GPL-3.0-only fails', !ok('GPL-3.0-only'));
  expect('MPL-2.0 fails (no permissive arm)', !ok('MPL-2.0'));
  expect('MIT AND LGPL-2.1-only fails', !ok('MIT AND LGPL-2.1-only'));
  expect('missing license (license-file only) fails', !ok(null));
  expect('empty license fails', !ok(''));
  expect('unknown identifier fails', !ok('Proprietary'));
  expect('unbalanced parentheses fail', !ok('(MIT OR Apache-2.0'));
  expect('dangling OR fails', !ok('MIT OR'));
  expect('WITH on a non-permissive base fails', !ok('GPL-2.0-only WITH Classpath-exception-2.0'));

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
  expect('third-party GPL crate fails', checkInventory(meta([ws, { id: 'g', name: 'g', version: '1.0.0', license: 'GPL-3.0-only', source: 'registry+https://github.com/rust-lang/crates.io-index' }])).violations.length === 1);
  const zrow = checkInventory(meta([ws, { id: 'z', name: 'zenoh', version: '1.10.1', license: 'EPL-2.0 OR Apache-2.0', source: 'registry+x' }]));
  expect('control: dual EPL/Apache crate passes and is flagged in the listing',
    zrow.violations.length === 0 && zrow.rows.some((r) => /elects Apache-2\.0\); copyleft arm flagged: EPL-2\.0/.test(r.verdict)));

  let failed = 0;
  for (const c of cases) {
    console.log(`${c.ok ? 'pass' : 'FAIL'}  ${c.name}`);
    if (!c.ok) failed++;
  }
  console.log(`self-test: ${cases.length - failed}/${cases.length} cases pass`);
  return failed ? 1 : 0;
}

function main(argv) {
  if (argv[0] === '--self-test') return selfTest();
  if (argv.length) {
    console.error('usage: check-licenses.mjs [--self-test]');
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
