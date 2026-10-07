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
//   tests/fakes/* -> core/ only        (#57: test doubles, never in a product build)
//   tests/protocol/contract/* -> core/ and tests/fakes/* only
//                                      (#59: the contract suites, never in a product build)
//
// What is checked, for every workspace member, over every dependency kind (normal, build
// and dev: cargo itself allows a dev-dependency cycle, so a core/ dev-dependency on an
// adapter would otherwise go unnoticed), every target platform, and every feature
// (`--all-features`, so an optional dependency behind a non-default feature is seen):
//   1. The member sits in the module layout: core, cli, adapters/<name> or
//      transports/<name>, or is a test double at tests/fakes/<name> (#57) or a contract
//      suite at tests/protocol/contract/<name> (#59). Anything else fails.
//   2. Reachability, not just direct edges: the member's transitive closure contains no
//      workspace crate the rule above forbids, whatever path (including through a
//      third-party crate) leads there.
//   3. Every adapter and transport has a normal dependency on core/.
//   5. Test doubles and contract suites stay out of every product build (#57, #59): no
//      product member (core, cli, an adapter or a transport) reaches a tests/fakes/<name>
//      or tests/protocol/contract/<name> crate over normal and build edges alone, so none
//      is compiled into the `oac` binary. An adapter, a transport or cli/ may take one as a
//      dev-dependency (rule 2 allows the reach); core/ may not, because core/ reaches
//      nothing in-repo.
//   4. Provider and transport crates stay with their owner: the zenoh crates may be a
//      direct dependency of transports/zenoh only, the Codex app-server crates of
//      adapters/codex only (07 section 5, "Consuming module"); and neither may be reachable
//      from any member other than its owner and cli/. So none is reachable from core/.
//      The device-key storage crates (keyring and its backends, age) are owned by cli/
//      the same way (#52, #315 review N-b).
//      Names match case-insensitively, with `_` folded to `-`.
//
//   node scripts/check-crate-deps.mjs                    # check this workspace
//   node scripts/check-crate-deps.mjs --metadata <file>  # check a saved metadata JSON
//   node scripts/check-crate-deps.mjs --self-test        # synthetic graphs, each planted
//                                                        # violation must fail
//   node scripts/check-crate-deps.mjs --mutation-test    # copy the real workspace to a
//                                                        # temp dir, plant violations in its
//                                                        # Cargo.toml files, run real cargo;
//                                                        # a cargo error counts as caught
//                                                        # only if it is a dependency cycle
//
// Node built-ins only; no cargo plugin. `cargo metadata` runs with --offline, so the check
// needs no network. Exit codes: 0 = clean; 1 = violation (or failed self/mutation test);
// 2 = usage or environment error.

import { spawnSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptPath = fileURLToPath(import.meta.url);
const repoRoot = resolve(dirname(scriptPath), '..');

// External crates owned by one module (07 section 5). Matched on the package name
// lowercased and with `_` folded to `-` (crates.io treats these spellings as the same
// name: Zenoh, zenoh_backend_traits, codex_app_server_protocol).
// The OS credential store and encrypted-file crates (#52; 07 section 5, consuming module
// cli/): keyring, keyring-core, every *-keyring-store backend, and the platform clients
// beneath them (secret-service, security-framework); age and age-core. Only cli/ builds the
// device-key stores, and core/ holds the key behind its KeyStore trait (#315 review N-b).
// The signature crates (ed25519-dalek and its curve crates) are core/'s own (C5 section 2)
// and stay unrestricted.
const OWNED_EXTERNAL = [
  { family: 'zenoh', re: /^zenoh(?:-|$)/, owner: 'transports/zenoh' },
  { family: 'Codex app-server', re: /^codex-app-server(?:-|$)/, owner: 'adapters/codex' },
  {
    family: 'OS credential store',
    re: /^(?:keyring(?:-core)?|[a-z0-9-]+-keyring-store|secret-service|security-framework(?:-sys)?)$/,
    owner: 'cli',
  },
  { family: 'encrypted-file key store', re: /^age(?:-core)?$/, owner: 'cli' },
];
export const ownedFamily = (name) => OWNED_EXTERNAL.find((o) => o.re.test(String(name).toLowerCase().replace(/_/g, '-')));

// Every cargo metadata call resolves with --all-features: an optional dependency behind a
// non-default feature is still a dependency the build can compile in, so it must be in
// the graph both checks read (#311 review B1).
export const METADATA_ARGS = ['metadata', '--format-version', '1', '--offline', '--all-features'];

// Module of a workspace member, from its manifest directory relative to the workspace root.
export function moduleOf(relDir) {
  const d = relDir.split(sep).join('/');
  if (d === 'core') return { kind: 'core', path: d };
  if (d === 'cli') return { kind: 'cli', path: d };
  let m = /^adapters\/([a-z0-9][a-z0-9_-]*)$/.exec(d);
  if (m) return { kind: 'adapter', path: d };
  m = /^transports\/([a-z0-9][a-z0-9_-]*)$/.exec(d);
  if (m) return { kind: 'transport', path: d };
  m = /^tests\/fakes\/([a-z0-9][a-z0-9_-]*)$/.exec(d);
  if (m) return { kind: 'fake', path: d };
  m = /^tests\/protocol\/contract\/([a-z0-9][a-z0-9_-]*)$/.exec(d);
  if (m) return { kind: 'suite', path: d };
  return null;
}

// May a member of kind `from` reach workspace member `to` (by module)?
// Over every edge kind; rule 5 separately keeps fakes off normal and build edges.
function allowedReach(from, to) {
  if (to.kind === 'cli') return false;
  if (from.kind === 'cli') return true;
  if (from.kind === 'core') return false;
  if (from.kind === 'fake') return to.kind === 'core'; // test doubles: core/ only
  if (from.kind === 'suite') return to.kind === 'core' || to.kind === 'fake'; // contract suites
  // adapters and transports: core/, and a fake or a suite as a dev-dependency (rule 5)
  return to.kind === 'core' || to.kind === 'fake' || to.kind === 'suite';
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
          '(core, cli, adapters/<name>, transports/<name>, tests/fakes/<name>, tests/protocol/contract/<name>)',
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
      const o = ownedFamily(pname);
      if (o && mod.path !== o.owner) {
        violations.push(`${name}: direct dependency on ${pname} (${o.family}); only ${o.owner} may depend on it`);
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
      const o = ownedFamily(pname);
      if (o && mod.path !== o.owner && mod.kind !== 'cli') {
        violations.push(`${name} reaches ${pname} (${o.family}, owned by ${o.owner}): ${pathTo(reached)}`);
      }
    }
    // 5. No test double in a product build: reachability over normal and build edges only.
    if (mod.kind !== 'fake' && mod.kind !== 'suite') {
      const built = new Map([[id, null]]);
      const q = [id];
      while (q.length) {
        const cur = q.shift();
        for (const d of depsOf(cur)) {
          if (built.has(d.id) || !d.kinds.some((k) => k.kind !== 'dev')) continue;
          built.set(d.id, cur);
          q.push(d.id);
        }
      }
      for (const reached of built.keys()) {
        const tmod = members.get(reached);
        if (tmod?.kind !== 'fake' && tmod?.kind !== 'suite') continue;
        const chain = [];
        for (let c = reached; c !== null; c = built.get(c)) chain.unshift(nameOf(c));
        const what = tmod.kind === 'fake' ? 'test double' : 'contract suite';
        violations.push(`${name} builds in ${what} ${tmod.path} (allowed only as a dev-dependency): ${chain.join(' -> ')}`);
      }
    }
  }
  return violations;
}

export function cargoMetadata(cwd, extra = []) {
  const r = spawnSync('cargo', [...METADATA_ARGS, ...extra], {
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
const FAKE_MEMBERS = { ...BASE_MEMBERS, 'oac-fake-claude': 'tests/fakes/claude' };
const FAKE_EDGES = [...BASE_EDGES, ['oac-fake-claude', 'oac-core']];
const SUITE_MEMBERS = {
  ...FAKE_MEMBERS,
  'oac-transport-memory': 'transports/memory',
  'oac-contract-transport': 'tests/protocol/contract/transport',
  'oac-contract-adapter': 'tests/protocol/contract/adapter',
};
const SUITE_EDGES = [
  ...FAKE_EDGES,
  ['oac-transport-memory', 'oac-core'],
  ['oac-contract-transport', 'oac-core'],
  ['oac-contract-adapter', 'oac-core'],
  ['oac-contract-adapter', 'oac-fake-claude'],
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
  // N1: `_` spellings of the owned families.
  { name: 'core depends on zenoh_backend_traits (underscore spelling)', meta: synth({ members: BASE_MEMBERS, externals: ['zenoh_backend_traits'], edges: [...BASE_EDGES, ['oac-core', 'zenoh_backend_traits']] }) },
  { name: 'adapter depends on codex_app_server_protocol outside adapters/codex', meta: synth({ members: BASE_MEMBERS, externals: ['codex_app_server_protocol'], edges: [...BASE_EDGES, ['oac-adapter-claude', 'codex_app_server_protocol']] }) },
  // N8: owner rules are case-insensitive.
  { name: 'core depends on a crate named Zenoh (mixed case)', meta: synth({ members: BASE_MEMBERS, externals: ['Zenoh'], edges: [...BASE_EDGES, ['oac-core', 'Zenoh']] }) },
  {
    name: 'control: adapters/codex may depend on codex_app_server_protocol',
    expectClean: true,
    meta: synth({ members: BASE_MEMBERS, externals: ['codex_app_server_protocol'], edges: [...BASE_EDGES, ['oac-adapter-codex', 'codex_app_server_protocol']] }),
  },
  // B1: an optional dependency behind a non-default feature. With --all-features (see the
  // METADATA_ARGS case below) cargo puts that edge in the resolve graph, as here.
  { name: 'optional feature-gated edge: adapters/claude -> adapters/codex', meta: synth({ members: BASE_MEMBERS, edges: [...BASE_EDGES, ['oac-adapter-claude', 'oac-adapter-codex']] }) },
  { name: 'optional feature-gated edge: core -> zenoh', meta: synth({ members: BASE_MEMBERS, externals: ['zenoh'], edges: [...BASE_EDGES, ['oac-core', 'zenoh']] }) },
  // #315 review N-b: the key-storage crates are cli/'s.
  {
    name: 'control: cli/ may depend on keyring and age; core/ on ed25519-dalek',
    expectClean: true,
    meta: synth({
      members: BASE_MEMBERS,
      externals: ['keyring', 'keyring-core', 'windows-native-keyring-store', 'age', 'age-core', 'ed25519-dalek'],
      edges: [...BASE_EDGES, ['oac-cli', 'keyring'], ['keyring', 'keyring-core'], ['keyring', 'windows-native-keyring-store'],
        ['oac-cli', 'age'], ['age', 'age-core'], ['oac-core', 'ed25519-dalek']],
    }),
  },
  { name: 'core depends on keyring', meta: synth({ members: BASE_MEMBERS, externals: ['keyring'], edges: [...BASE_EDGES, ['oac-core', 'keyring']] }) },
  { name: 'core depends on age', meta: synth({ members: BASE_MEMBERS, externals: ['age'], edges: [...BASE_EDGES, ['oac-core', 'age']] }) },
  {
    name: 'adapter reaches zbus-secret-service-keyring-store transitively',
    meta: synth({
      members: BASE_MEMBERS,
      externals: ['helper', 'zbus-secret-service-keyring-store'],
      edges: [...BASE_EDGES, ['oac-adapter-claude', 'helper'], ['helper', 'zbus-secret-service-keyring-store']],
    }),
  },
  { name: 'transport depends on security-framework', meta: synth({ members: BASE_MEMBERS, externals: ['security-framework'], edges: [...BASE_EDGES, ['oac-transport-zenoh', 'security-framework']] }) },
  { name: 'core depends on keyring_core (underscore spelling)', meta: synth({ members: BASE_MEMBERS, externals: ['keyring_core'], edges: [...BASE_EDGES, ['oac-core', 'keyring_core']] }) },
  // #57: test doubles at tests/fakes/<name> stay out of every product build.
  {
    name: 'control: a fake depends on core/; an adapter, a transport and cli/ dev-depend on it',
    expectClean: true,
    meta: synth({
      members: FAKE_MEMBERS,
      edges: [...FAKE_EDGES, ['oac-adapter-claude', 'oac-fake-claude', 'dev'], ['oac-transport-zenoh', 'oac-fake-claude', 'dev'],
        ['oac-cli', 'oac-fake-claude', 'dev']],
    }),
  },
  { name: 'cli -> fake (normal)', meta: synth({ members: FAKE_MEMBERS, edges: [...FAKE_EDGES, ['oac-cli', 'oac-fake-claude']] }) },
  { name: 'adapter -> fake (normal)', meta: synth({ members: FAKE_MEMBERS, edges: [...FAKE_EDGES, ['oac-adapter-claude', 'oac-fake-claude']] }) },
  { name: 'transport -> fake (build-dependency)', meta: synth({ members: FAKE_MEMBERS, edges: [...FAKE_EDGES, ['oac-transport-zenoh', 'oac-fake-claude', 'build']] }) },
  { name: 'core -> fake (dev-dependency)', meta: synth({ members: FAKE_MEMBERS, edges: [...FAKE_EDGES, ['oac-core', 'oac-fake-claude', 'dev']] }) },
  {
    name: 'adapter builds in a fake through a third-party crate',
    meta: synth({ members: FAKE_MEMBERS, externals: ['shim'], edges: [...FAKE_EDGES, ['oac-adapter-codex', 'shim'], ['shim', 'oac-fake-claude']] }),
  },
  { name: 'fake -> adapter', meta: synth({ members: FAKE_MEMBERS, edges: [...FAKE_EDGES, ['oac-fake-claude', 'oac-adapter-claude']] }) },
  { name: 'fake -> cli (dev-dependency)', meta: synth({ members: FAKE_MEMBERS, edges: [...FAKE_EDGES, ['oac-fake-claude', 'oac-cli', 'dev']] }) },
  { name: 'fake depends on zenoh', meta: synth({ members: FAKE_MEMBERS, externals: ['zenoh'], edges: [...FAKE_EDGES, ['oac-fake-claude', 'zenoh']] }) },
  { name: 'fake outside tests/fakes/<name> (tests/claude)', meta: synth({ members: { ...BASE_MEMBERS, 'oac-fake-claude': 'tests/claude' }, edges: FAKE_EDGES }) },
  { name: 'nested fake path (tests/fakes/claude/inner)', meta: synth({ members: { ...BASE_MEMBERS, 'oac-fake-claude': 'tests/fakes/claude/inner' }, edges: FAKE_EDGES }) },
  // #59: the contract suites at tests/protocol/contract/<name> stay out of every product build.
  {
    name: 'control: suites depend on core/ and a fake; a transport, an adapter and cli/ dev-depend on them',
    expectClean: true,
    meta: synth({
      members: SUITE_MEMBERS,
      edges: [...SUITE_EDGES, ['oac-transport-memory', 'oac-contract-transport', 'dev'],
        ['oac-adapter-claude', 'oac-contract-adapter', 'dev'], ['oac-cli', 'oac-contract-adapter', 'dev']],
    }),
  },
  { name: 'transport -> suite (normal)', meta: synth({ members: SUITE_MEMBERS, edges: [...SUITE_EDGES, ['oac-transport-memory', 'oac-contract-transport']] }) },
  { name: 'cli -> suite (build-dependency)', meta: synth({ members: SUITE_MEMBERS, edges: [...SUITE_EDGES, ['oac-cli', 'oac-contract-adapter', 'build']] }) },
  { name: 'core -> suite (dev-dependency)', meta: synth({ members: SUITE_MEMBERS, edges: [...SUITE_EDGES, ['oac-core', 'oac-contract-transport', 'dev']] }) },
  { name: 'suite -> transport', meta: synth({ members: SUITE_MEMBERS, edges: [...SUITE_EDGES, ['oac-contract-transport', 'oac-transport-memory']] }) },
  { name: 'suite -> adapter (dev-dependency)', meta: synth({ members: SUITE_MEMBERS, edges: [...SUITE_EDGES, ['oac-contract-adapter', 'oac-adapter-claude', 'dev']] }) },
  { name: 'suite -> sibling suite', meta: synth({ members: SUITE_MEMBERS, edges: [...SUITE_EDGES, ['oac-contract-adapter', 'oac-contract-transport']] }) },
  { name: 'fake -> suite', meta: synth({ members: SUITE_MEMBERS, edges: [...SUITE_EDGES, ['oac-fake-claude', 'oac-contract-adapter']] }) },
  { name: 'suite depends on zenoh', meta: synth({ members: SUITE_MEMBERS, externals: ['zenoh'], edges: [...SUITE_EDGES, ['oac-contract-transport', 'zenoh']] }) },
  { name: 'suite outside tests/protocol/contract/<name> (tests/contract/x)', meta: synth({ members: { ...SUITE_MEMBERS, 'oac-contract-transport': 'tests/contract/transport' }, edges: SUITE_EDGES }) },
  {
    name: 'adapter builds in a suite through a third-party crate',
    meta: synth({ members: SUITE_MEMBERS, externals: ['shim'], edges: [...SUITE_EDGES, ['oac-adapter-codex', 'shim'], ['shim', 'oac-contract-adapter']] }),
  },
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
  // B1: every metadata call must resolve optional dependencies too.
  const allFeatures = METADATA_ARGS.includes('--all-features');
  console.log(`${allFeatures ? 'pass' : 'FAIL'}  cargo metadata runs with --all-features (optional dependencies are in the graph)`);
  if (!allFeatures) failed++;
  const total = SELF_TEST_CASES.length + 1;
  console.log(`self-test: ${total - failed}/${total} cases pass`);
  return failed ? 1 : 0;
}

// ---------------------------------------------------------------------------------------
// --mutation-test: plant violations in a copy of the real workspace and run real cargo.

const addLine = (section, line, text) =>
  text.includes(`[${section}]\n`) ? text.replace(`[${section}]\n`, `[${section}]\n${line}\n`) : `${text}\n[${section}]\n${line}\n`;
export const addDep = (section, line) => (text) => addLine(section, line, text);
// An optional dependency behind a non-default feature (`leak`), which a default-feature
// resolve would not see (#311 review B1).
export const addOptionalDep = (line) => (text) => addLine('features', 'leak = ["dep:stub"]', addLine('dependencies', line, text));

// Stub crates outside the workspace root (so they are not implicit members), referenced
// from mutated manifests as `<ws>/../stubs/<dir>`.
export const STUBS = {
  zenoh: { name: 'zenoh', license: 'EPL-2.0 OR Apache-2.0' },
  'zenoh-backend-traits': { name: 'zenoh_backend_traits', license: 'EPL-2.0 OR Apache-2.0' },
  'codex-app-server-protocol': { name: 'codex_app_server_protocol', license: 'Apache-2.0' },
  'gpl-stub': { name: 'gpl-stub', license: 'GPL-3.0-only' },
  keyring: { name: 'keyring', license: 'MIT OR Apache-2.0' },
  age: { name: 'age', license: 'MIT OR Apache-2.0' },
};

// Copy the real workspace (manifests, lockfile, toolchain file, member sources) to
// <tmp>/ws, write the stubs to <tmp>/stubs, apply `edit` to one manifest, run `fn(wsDir)`,
// and always remove the temp dir.
export function withWorkspaceCopy(edit, fn) {
  const real = cargoMetadata(repoRoot, ['--no-deps']);
  if (real.status !== 0) throw new Error(`cargo metadata failed: ${real.stderr}`);
  const meta = JSON.parse(real.stdout);
  const memberDirs = meta.packages
    .filter((p) => meta.workspace_members.includes(p.id))
    .map((p) => relative(meta.workspace_root, dirname(p.manifest_path)));
  const tmp = mkdtempSync(join(tmpdir(), 'oac-crate-deps-'));
  try {
    const ws = join(tmp, 'ws');
    for (const f of ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml']) {
      if (existsSync(join(repoRoot, f))) cpSync(join(repoRoot, f), join(ws, f));
    }
    for (const d of memberDirs) {
      cpSync(join(repoRoot, d, 'Cargo.toml'), join(ws, d, 'Cargo.toml'));
      cpSync(join(repoRoot, d, 'src'), join(ws, d, 'src'), { recursive: true });
    }
    for (const [dir, s] of Object.entries(STUBS)) {
      mkdirSync(join(tmp, 'stubs', dir, 'src'), { recursive: true });
      writeFileSync(join(tmp, 'stubs', dir, 'Cargo.toml'),
        `[package]\nname = "${s.name}"\nversion = "0.0.1"\nedition = "2024"\nlicense = "${s.license}"\n`);
      writeFileSync(join(tmp, 'stubs', dir, 'src', 'lib.rs'), '');
    }
    if (edit) {
      const p = join(ws, edit.file);
      const before = readFileSync(p, 'utf8').replace(/\r\n/g, '\n');
      const after = edit.edit(before);
      if (after === before) throw new Error(`mutation "${edit.name}" did not change ${edit.file}`);
      writeFileSync(p, after);
    }
    return fn(ws);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
}

const MUTATIONS = [
  { name: 'core/ depends on transports/zenoh', file: 'core/Cargo.toml', edit: addDep('dependencies', 'oac-transport-zenoh = { path = "../transports/zenoh" }') },
  { name: 'core/ dev-depends on adapters/claude', file: 'core/Cargo.toml', edit: addDep('dev-dependencies', 'oac-adapter-claude = { path = "../adapters/claude" }') },
  { name: 'adapters/claude depends on transports/zenoh', file: 'adapters/claude/Cargo.toml', edit: addDep('dependencies', 'oac-transport-zenoh = { path = "../../transports/zenoh" }') },
  { name: 'adapters/codex depends on adapters/claude', file: 'adapters/codex/Cargo.toml', edit: addDep('dependencies', 'oac-adapter-claude = { path = "../claude" }') },
  { name: 'transports/zenoh depends on cli/', file: 'transports/zenoh/Cargo.toml', edit: addDep('dependencies', 'oac-cli = { path = "../../cli" }') },
  { name: 'adapters/claude drops its core/ dependency', file: 'adapters/claude/Cargo.toml', edit: (t) => t.replace(/^oac-core\.workspace = true\n/m, '') },
  // B1: optional dependencies behind a non-default feature.
  {
    name: 'adapters/claude optionally depends on adapters/codex (feature "leak")',
    file: 'adapters/claude/Cargo.toml',
    edit: addOptionalDep('stub = { package = "oac-adapter-codex", path = "../codex", optional = true }'),
  },
  {
    name: 'core/ optionally depends on zenoh (feature "leak")',
    file: 'core/Cargo.toml',
    edit: addOptionalDep('stub = { package = "zenoh", path = "../../stubs/zenoh", optional = true }'),
  },
  // N1: underscore spellings of the owned families.
  { name: 'core/ depends on zenoh_backend_traits', file: 'core/Cargo.toml', edit: addDep('dependencies', 'zenoh_backend_traits = { path = "../../stubs/zenoh-backend-traits" }') },
  { name: 'adapters/claude depends on codex_app_server_protocol', file: 'adapters/claude/Cargo.toml', edit: addDep('dependencies', 'codex_app_server_protocol = { path = "../../../stubs/codex-app-server-protocol" }') },
  // #315 review N-b: the key-storage crates are cli/'s.
  { name: 'core/ depends on keyring', file: 'core/Cargo.toml', edit: addDep('dependencies', 'keyring = { path = "../../stubs/keyring" }') },
  { name: 'adapters/codex depends on age', file: 'adapters/codex/Cargo.toml', edit: addDep('dependencies', 'age = { path = "../../../stubs/age" }') },
  // #57: the fake Claude endpoint is never a normal dependency of a product crate.
  { name: 'cli/ depends on tests/fakes/claude', file: 'cli/Cargo.toml', edit: addDep('dependencies', 'oac-fake-claude = { path = "../tests/fakes/claude" }') },
  { name: 'adapters/claude depends on tests/fakes/claude', file: 'adapters/claude/Cargo.toml', edit: addDep('dependencies', 'oac-fake-claude = { path = "../../tests/fakes/claude" }') },
  { name: 'core/ dev-depends on tests/fakes/claude', file: 'core/Cargo.toml', edit: addDep('dev-dependencies', 'oac-fake-claude = { path = "../tests/fakes/claude" }') },
  { name: 'tests/fakes/claude depends on adapters/claude', file: 'tests/fakes/claude/Cargo.toml', edit: addDep('dependencies', 'oac-adapter-claude = { path = "../../../adapters/claude" }') },
  // #59: the contract suites are never a normal dependency of a product crate.
  { name: 'transports/memory depends on the transport suite', file: 'transports/memory/Cargo.toml', edit: addDep('dependencies', 'oac-contract-transport = { path = "../../tests/protocol/contract/transport" }') },
  { name: 'cli/ depends on the adapter suite', file: 'cli/Cargo.toml', edit: addDep('dependencies', 'oac-contract-adapter = { path = "../tests/protocol/contract/adapter" }') },
  { name: 'the transport suite depends on transports/memory', file: 'tests/protocol/contract/transport/Cargo.toml', edit: addDep('dependencies', 'oac-transport-memory = { path = "../../../../transports/memory" }') },
];

// Only cargo's cycle error counts as cargo catching a planted edge; any other cargo error
// means the mutation itself is malformed, and the case fails (#311 review N4).
const CARGO_CYCLE = 'cyclic package dependency';

function mutationTest() {
  const runCheck = (dir) => {
    const m = cargoMetadata(dir);
    if (m.status !== 0) {
      const line = (m.stderr.split(/\r?\n/).find((l) => /error/i.test(l)) ?? 'cargo metadata failed').trim();
      if (m.stderr.includes(CARGO_CYCLE)) return { failed: true, by: `cargo refused the manifest: ${line}` };
      return { failed: false, malformed: true, by: `cargo error that is not a cycle (malformed mutation?): ${line}` };
    }
    const v = checkMetadata(JSON.parse(m.stdout));
    return v.length ? { failed: true, by: `checker: ${v[0]}${v.length > 1 ? ` (+${v.length - 1} more)` : ''}` } : { failed: false };
  };
  let bad = 0;
  const cases = [{ name: 'control: unmodified copy of the workspace', control: true }, ...MUTATIONS];
  for (const c of cases) {
    const r = withWorkspaceCopy(c.control ? null : c, runCheck);
    const ok = !r.malformed && (c.control ? !r.failed : r.failed);
    if (!ok) bad++;
    console.log(`${ok ? 'pass' : 'FAIL'}  ${c.name}${r.by ? ` -- ${r.by}` : ''}`);
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

