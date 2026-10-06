#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
//
// Enforce the workspace dependency direction (#50, F1) over the resolved `cargo metadata`
// graph. The rule is docs/planning/v0.1/07-repository-and-dependencies.md section 3
// (cli/ edges widened by #305), restated in oac-implementation section 2:
//
//   cli/          -> core/, adapters/*, transports/*   (construction only)
//   adapters/*    -> core/ only        (never another adapter, a transport, or cli/)
//   transports/*  -> core/ only        (never another transport, an adapter, or cli/)
//   core/         -> nothing in-repo
//   (nothing)     -> cli/
//
// What is checked, for every workspace member, over every dependency kind (normal, build
// and dev: cargo itself allows a dev-dependency cycle, so a core/ dev-dependency on an
// adapter would otherwise go unnoticed) and every target platform:
//   1. The member sits in the module layout: core, cli, adapters/<name> or
//      transports/<name>. Anything else fails.
//   2. Reachability, not just direct edges: the member's transitive closure contains no
//      workspace crate the rule above forbids, whatever path (including through a
//      third-party crate) leads there.
//   3. Every adapter and transport has a normal dependency on core/.
//   4. Provider and transport crates stay with their owner: the zenoh crates may be a
//      direct dependency of transports/zenoh only, the Codex app-server crates of
//      adapters/codex only (07 section 5, "Consuming module"); and neither may be reachable
//      from any member other than its owner and cli/. So none is reachable from core/.
//
//   node scripts/check-crate-deps.mjs                    # check this workspace
//   node scripts/check-crate-deps.mjs --metadata <file>  # check a saved metadata JSON
//   node scripts/check-crate-deps.mjs --self-test        # synthetic graphs, each planted
//                                                        # violation must fail
//   node scripts/check-crate-deps.mjs --mutation-test    # copy the real workspace to a
//                                                        # temp dir, plant violations in its
//                                                        # Cargo.toml files, run real cargo
//
// Node built-ins only; no cargo plugin. `cargo metadata` runs with --offline, so the check
// needs no network. Exit codes: 0 = clean; 1 = violation (or failed self/mutation test);
// 2 = usage or environment error.

import { spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptPath = fileURLToPath(import.meta.url);
const repoRoot = resolve(dirname(scriptPath), '..');

// External crates owned by one module (07 section 5). Matched on the package name.
const OWNED_EXTERNAL = [
  { family: 'zenoh', re: /^zenoh(?:-|$)/, owner: 'transports/zenoh' },
  { family: 'Codex app-server', re: /^codex-app-server(?:-|$)/, owner: 'adapters/codex' },
];

// Module of a workspace member, from its manifest directory relative to the workspace root.
export function moduleOf(relDir) {
  const d = relDir.split(sep).join('/');
  if (d === 'core') return { kind: 'core', path: d };
  if (d === 'cli') return { kind: 'cli', path: d };
  let m = /^adapters\/([a-z0-9][a-z0-9_-]*)$/.exec(d);
  if (m) return { kind: 'adapter', path: d };
  m = /^transports\/([a-z0-9][a-z0-9_-]*)$/.exec(d);
  if (m) return { kind: 'transport', path: d };
  return null;
}

// May a member of kind `from` reach workspace member `to` (by module)?
function allowedReach(from, to) {
  if (to.kind === 'cli') return false;
  if (from.kind === 'cli') return true;
  if (from.kind === 'core') return false;
  return to.kind === 'core'; // adapters and transports: core/ only
}

// Pure check over a parsed `cargo metadata --format-version 1` document. Returns a list of
// violation strings (empty = clean).
export function checkMetadata(meta) {
  const violations = [];
  const root = meta.workspace_root;
  const pkgById = new Map(meta.packages.map((p) => [p.id, p]));
  const members = new Map(); // id -> module
  for (const id of meta.workspace_members) {
    const p = pkgById.get(id);
    if (!p) {
      violations.push(`workspace member ${id} missing from packages`);
      continue;
    }
    const rel = relative(root, dirname(p.manifest_path));
    const mod = moduleOf(rel);
    if (!mod) {
      violations.push(
        `${p.name} (${rel.split(sep).join('/') || '.'}): workspace member outside the module layout ` +
          '(core, cli, adapters/<name>, transports/<name>)',
      );
      continue;
    }
    members.set(id, mod);
  }
  if (!meta.resolve) {
    violations.push('metadata has no resolve graph (was it run with --no-deps?)');
    return violations;
  }
  const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
  const nameOf = (id) => {
    const m = members.get(id);
    return m ? `${pkgById.get(id)?.name} (${m.path})` : pkgById.get(id)?.name ?? id;
  };
  const depsOf = (id) => (nodes.get(id)?.deps ?? []).map((d) => ({ id: d.pkg, kinds: d.dep_kinds ?? [] }));

  for (const [id, mod] of members) {
    const name = nameOf(id);
    // 3. adapters and transports depend on core/ (normal edge).
    if (mod.kind === 'adapter' || mod.kind === 'transport') {
      const hasCore = depsOf(id).some(
        (d) => members.get(d.id)?.kind === 'core' && d.kinds.some((k) => k.kind === null || k.kind === 'normal'),
      );
      if (!hasCore) violations.push(`${name}: no normal dependency on core/ (adapters and transports depend on core)`);
    }
    // 4a. owned external crates as direct dependencies.
    for (const d of depsOf(id)) {
      if (members.has(d.id)) continue;
      const pname = pkgById.get(d.id)?.name ?? '';
      for (const o of OWNED_EXTERNAL) {
        if (o.re.test(pname) && mod.path !== o.owner) {
          violations.push(`${name}: direct dependency on ${pname} (${o.family}); only ${o.owner} may depend on it`);
        }
      }
    }
    // 2 and 4b. reachability (BFS with parent links, for a readable path).
    const parent = new Map([[id, null]]);
    const queue = [id];
    while (queue.length) {
      const cur = queue.shift();
      for (const d of depsOf(cur)) {
        if (parent.has(d.id)) continue;
        parent.set(d.id, cur);
        queue.push(d.id);
      }
    }
    const pathTo = (t) => {
      const chain = [];
      for (let c = t; c !== null; c = parent.get(c)) chain.unshift(nameOf(c));
      return chain.join(' -> ');
    };
    for (const reached of parent.keys()) {
      if (reached === id) continue;
      const tmod = members.get(reached);
      if (tmod) {
        if (!allowedReach(mod, tmod)) violations.push(`${name} reaches ${tmod.path}: ${pathTo(reached)}`);
        continue;
      }
      const pname = pkgById.get(reached)?.name ?? '';
      for (const o of OWNED_EXTERNAL) {
        if (o.re.test(pname) && mod.path !== o.owner && mod.kind !== 'cli') {
          violations.push(`${name} reaches ${pname} (${o.family}, owned by ${o.owner}): ${pathTo(reached)}`);
        }
      }
    }
  }
  return violations;
}

function cargoMetadata(cwd, extra = []) {
  const r = spawnSync('cargo', ['metadata', '--format-version', '1', '--offline', ...extra], {
    cwd,
    encoding: 'utf8',
    maxBuffer: 256 * 1024 * 1024,
  });
  if (r.error) throw r.error;
  return { status: r.status, stdout: r.stdout, stderr: r.stderr };
}

function report(violations, label) {
  if (violations.length === 0) {
    console.log(`crate dependency direction: CLEAN (${label})`);
    return 0;
  }
  for (const v of violations) console.log(`FAIL  ${v}`);
  console.log(`crate dependency direction: FAIL -- ${violations.length} violation(s) (${label}). ` +
    'Stop and cite the boundary (oac-implementation section 2, oac-boundaries).');
  return 1;
}

// ---------------------------------------------------------------------------------------
// --self-test: synthetic metadata documents.

function synth({ members, externals = [], edges }) {
  // members: { name: relDir }; externals: [name]; edges: [[from, to, kind?]]
  const root = resolve('/ws');
  const id = (n) => `id:${n}`;
  const packages = [
    ...Object.entries(members).map(([n, d]) => ({ id: id(n), name: n, manifest_path: join(root, d, 'Cargo.toml') })),
    ...externals.map((n) => ({ id: id(n), name: n, manifest_path: join(root, '..', 'registry', n, 'Cargo.toml') })),
  ];
  const all = [...Object.keys(members), ...externals];
  const nodes = all.map((n) => ({
    id: id(n),
    deps: edges
      .filter(([f]) => f === n)
      .map(([, t, k = null]) => ({ name: t, pkg: id(t), dep_kinds: [{ kind: k, target: null }] })),
  }));
  return { workspace_root: root, workspace_members: Object.keys(members).map(id), packages, resolve: { nodes } };
}

const BASE_MEMBERS = {
  'oac-core': 'core',
  'oac-cli': 'cli',
  'oac-adapter-claude': 'adapters/claude',
  'oac-adapter-codex': 'adapters/codex',
  'oac-transport-zenoh': 'transports/zenoh',
};
const BASE_EDGES = [
  ['oac-adapter-claude', 'oac-core'],
  ['oac-adapter-codex', 'oac-core'],
  ['oac-transport-zenoh', 'oac-core'],
  ['oac-cli', 'oac-core'],
  ['oac-cli', 'oac-adapter-claude'],
  ['oac-cli', 'oac-adapter-codex'],
  ['oac-cli', 'oac-transport-zenoh'],
];
const without = (edges, f, t) => edges.filter(([a, b]) => !(a === f && b === t));

const SELF_TEST_CASES = [
  { name: 'control: the scaffold graph is clean', expectClean: true, meta: synth({ members: BASE_MEMBERS, edges: BASE_EDGES }) },
  {
    name: 'control: owners may depend on their own external crates; cli may reach them',
    expectClean: true,
    meta: synth({
      members: BASE_MEMBERS,
      externals: ['zenoh', 'zenoh-protocol', 'codex-app-server-client', 'serde'],
      edges: [...BASE_EDGES, ['oac-transport-zenoh', 'zenoh'], ['zenoh', 'zenoh-protocol'],
        ['oac-adapter-codex', 'codex-app-server-client'], ['oac-core', 'serde']],
    }),
  },
  { name: 'core -> transport (normal)', meta: synth({ members: BASE_MEMBERS, edges: [...BASE_EDGES, ['oac-core', 'oac-transport-zenoh']] }) },
  { name: 'core -> adapter (dev-dependency)', meta: synth({ members: BASE_MEMBERS, edges: [...BASE_EDGES, ['oac-core', 'oac-adapter-claude', 'dev']] }) },
  { name: 'core -> cli (build-dependency)', meta: synth({ members: BASE_MEMBERS, edges: [...BASE_EDGES, ['oac-core', 'oac-cli', 'build']] }) },
  { name: 'adapter -> sibling adapter', meta: synth({ members: BASE_MEMBERS, edges: [...BASE_EDGES, ['oac-adapter-claude', 'oac-adapter-codex']] }) },
  { name: 'adapter -> transport', meta: synth({ members: BASE_MEMBERS, edges: [...BASE_EDGES, ['oac-adapter-codex', 'oac-transport-zenoh']] }) },
  { name: 'adapter -> cli', meta: synth({ members: BASE_MEMBERS, edges: [...BASE_EDGES, ['oac-adapter-claude', 'oac-cli']] }) },
  {
    name: 'transport -> sibling transport',
    meta: synth({
      members: { ...BASE_MEMBERS, 'oac-transport-memory': 'transports/memory' },
      edges: [...BASE_EDGES, ['oac-transport-memory', 'oac-core'], ['oac-transport-zenoh', 'oac-transport-memory']],
    }),
  },
  {
    name: 'core reaches a transport through a third-party crate',
    meta: synth({
      members: BASE_MEMBERS,
      externals: ['shim'],
      edges: [...BASE_EDGES, ['oac-core', 'shim'], ['shim', 'oac-transport-zenoh']],
    }),
  },
  {
    name: 'core reaches the zenoh crate transitively',
    meta: synth({ members: BASE_MEMBERS, externals: ['helper', 'zenoh'], edges: [...BASE_EDGES, ['oac-core', 'helper'], ['helper', 'zenoh']] }),
  },
  { name: 'core depends on a Codex app-server crate', meta: synth({ members: BASE_MEMBERS, externals: ['codex-app-server-protocol'], edges: [...BASE_EDGES, ['oac-core', 'codex-app-server-protocol']] }) },
  { name: 'cli depends on zenoh directly', meta: synth({ members: BASE_MEMBERS, externals: ['zenoh'], edges: [...BASE_EDGES, ['oac-cli', 'zenoh']] }) },
  { name: 'adapter depends on zenoh directly', meta: synth({ members: BASE_MEMBERS, externals: ['zenoh'], edges: [...BASE_EDGES, ['oac-adapter-claude', 'zenoh']] }) },
  { name: 'adapter without a core/ dependency', meta: synth({ members: BASE_MEMBERS, edges: without(BASE_EDGES, 'oac-adapter-codex', 'oac-core') }) },
  {
    name: 'transport with only a dev-dependency on core/',
    meta: synth({ members: BASE_MEMBERS, edges: [...without(BASE_EDGES, 'oac-transport-zenoh', 'oac-core'), ['oac-transport-zenoh', 'oac-core', 'dev']] }),
  },
  { name: 'workspace member outside the module layout', meta: synth({ members: { ...BASE_MEMBERS, 'oac-extra': 'tools/extra' }, edges: BASE_EDGES }) },
  { name: 'nested module path (adapters/claude/inner)', meta: synth({ members: { ...BASE_MEMBERS, 'oac-inner': 'adapters/claude/inner' }, edges: [...BASE_EDGES, ['oac-inner', 'oac-core']] }) },
];

function selfTest() {
  let failed = 0;
  for (const c of SELF_TEST_CASES) {
    const v = checkMetadata(c.meta);
    const ok = c.expectClean ? v.length === 0 : v.length > 0;
    console.log(`${ok ? 'pass' : 'FAIL'}  ${c.name}${c.expectClean ? '' : ` (${v.length} violation(s))`}`);
    if (!ok) {
      failed++;
      for (const x of v) console.log(`        ${x}`);
    }
  }
  console.log(`self-test: ${SELF_TEST_CASES.length - failed}/${SELF_TEST_CASES.length} cases pass`);
  return failed ? 1 : 0;
}

// ---------------------------------------------------------------------------------------
// --mutation-test: plant violations in a copy of the real workspace and run real cargo.

const addDep = (section, line) => (text) =>
  text.includes(`[${section}]`) ? text.replace(`[${section}]\n`, `[${section}]\n${line}\n`) : `${text}\n[${section}]\n${line}\n`;

const MUTATIONS = [
  { name: 'core/ depends on transports/zenoh', file: 'core/Cargo.toml', edit: addDep('dependencies', 'oac-transport-zenoh = { path = "../transports/zenoh" }') },
  { name: 'core/ dev-depends on adapters/claude', file: 'core/Cargo.toml', edit: addDep('dev-dependencies', 'oac-adapter-claude = { path = "../adapters/claude" }') },
  { name: 'adapters/claude depends on transports/zenoh', file: 'adapters/claude/Cargo.toml', edit: addDep('dependencies', 'oac-transport-zenoh = { path = "../../transports/zenoh" }') },
  { name: 'adapters/codex depends on adapters/claude', file: 'adapters/codex/Cargo.toml', edit: addDep('dependencies', 'oac-adapter-claude = { path = "../claude" }') },
  { name: 'transports/zenoh depends on cli/', file: 'transports/zenoh/Cargo.toml', edit: addDep('dependencies', 'oac-cli = { path = "../../cli" }') },
  { name: 'adapters/claude drops its core/ dependency', file: 'adapters/claude/Cargo.toml', edit: (t) => t.replace(/^oac-core\.workspace = true\n/m, '') },
];

function mutationTest() {
  const real = cargoMetadata(repoRoot, ['--no-deps']);
  if (real.status !== 0) {
    console.error(real.stderr);
    return 2;
  }
  const meta = JSON.parse(real.stdout);
  const memberDirs = meta.packages
    .filter((p) => meta.workspace_members.includes(p.id))
    .map((p) => relative(meta.workspace_root, dirname(p.manifest_path)));
  const copyWorkspace = (dest) => {
    for (const f of ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml']) {
      if (existsSync(join(repoRoot, f))) cpSync(join(repoRoot, f), join(dest, f));
    }
    for (const d of memberDirs) {
      cpSync(join(repoRoot, d, 'Cargo.toml'), join(dest, d, 'Cargo.toml'));
      cpSync(join(repoRoot, d, 'src'), join(dest, d, 'src'), { recursive: true });
    }
  };
  const runCheck = (dir) => {
    const m = cargoMetadata(dir);
    if (m.status !== 0) {
      const why = (m.stderr.split(/\r?\n/).find((l) => /error/i.test(l)) ?? 'cargo metadata failed').trim();
      return { failed: true, by: `cargo refused the manifest: ${why}` };
    }
    const v = checkMetadata(JSON.parse(m.stdout));
    return v.length ? { failed: true, by: `checker: ${v[0]}${v.length > 1 ? ` (+${v.length - 1} more)` : ''}` } : { failed: false };
  };
  let bad = 0;
  const cases = [{ name: 'control: unmodified copy of the workspace', control: true }, ...MUTATIONS];
  for (const c of cases) {
    const dir = mkdtempSync(join(tmpdir(), 'oac-crate-deps-'));
    try {
      copyWorkspace(dir);
      if (!c.control) {
        const p = join(dir, c.file);
        const before = readFileSync(p, 'utf8').replace(/\r\n/g, '\n');
        const after = c.edit(before);
        if (after === before) throw new Error(`mutation "${c.name}" did not change ${c.file}`);
        writeFileSync(p, after);
      }
      const r = runCheck(dir);
      const ok = c.control ? !r.failed : r.failed;
      if (!ok) bad++;
      console.log(`${ok ? 'pass' : 'FAIL'}  ${c.name}${r.by ? ` -- ${r.by}` : ''}`);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }
  console.log(`mutation test: ${cases.length - bad}/${cases.length} cases pass`);
  return bad ? 1 : 0;
}

// ---------------------------------------------------------------------------------------

function main(argv) {
  if (argv[0] === '--self-test') return selfTest();
  if (argv[0] === '--mutation-test') return mutationTest();
  if (argv[0] === '--metadata') {
    if (!argv[1]) {
      console.error('usage: --metadata <file>');
      return 2;
    }
    return report(checkMetadata(JSON.parse(readFileSync(argv[1], 'utf8'))), argv[1]);
  }
  if (argv.length) {
    console.error('usage: check-crate-deps.mjs [--self-test | --mutation-test | --metadata <file>]');
    return 2;
  }
  const r = cargoMetadata(repoRoot, ['--locked']);
  if (r.status !== 0) {
    console.error(r.stderr);
    return 2;
  }
  const meta = JSON.parse(r.stdout);
  return report(checkMetadata(meta), `${meta.workspace_members.length} workspace members, ${meta.packages.length} packages`);
}

if (process.argv[1] && resolve(process.argv[1]) === scriptPath) {
  try {
    process.exitCode = main(process.argv.slice(2));
  } catch (e) {
    console.error(e?.stack ?? String(e));
    process.exitCode = 2;
  }
}

