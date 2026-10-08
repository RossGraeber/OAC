#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
//
// Enforce the workspace dependency direction (#50, F1) over the resolved `cargo metadata`
// graph. The rule is docs/planning/v0.1/07-repository-and-dependencies.md section 3
// (cli/ edges widened by #305), restated in oac-implementation section 2:
//
//   cli/          -> core/, adapters/*, transports/*   (construction only)
//   adapters/*    -> core/ and adapters/mcp-tools only (never another adapter, a transport,
//                                      or cli/)
//   adapters/mcp-tools -> core/ only   (#7: the MCP tool surface both adapters share; its own
//                                      module kind, not an adapter, G-7 section 4)
//   transports/*  -> core/ only        (never another transport, an adapter, or cli/)
//   core/         -> nothing in-repo
//   (nothing)     -> cli/
//   tests/fakes/* -> core/ only        (#57: test doubles, never in a product build)
//   tests/protocol/contract/* -> core/ and tests/fakes/* only
//                                      (#59: the contract suites, never in a product build)
//   tests/security -> core/, tests/fakes/*, transports/* and tests/protocol/contract/* only
//                                      (#60: the security suite; a leaf, nothing reaches it)
//
// What is checked, for every workspace member, over every dependency kind (normal, build
// and dev: cargo itself allows a dev-dependency cycle, so a core/ dev-dependency on an
// adapter would otherwise go unnoticed), every target platform, and every feature
// (`--all-features`, so an optional dependency behind a non-default feature is seen):
//   1. The member sits in the module layout: core, cli, adapters/<name>, the shared tool
//      crate adapters/mcp-tools (#7), or transports/<name>, or is a test double at tests/fakes/<name> (#57) or a contract
//      suite at tests/protocol/contract/<name> (#59), or is the security suite at
//      tests/security (#60). Anything else fails.
//   2. Reachability, not just direct edges: the member's transitive closure contains no
//      workspace crate the rule above forbids, whatever path (including through a
//      third-party crate) leads there.
//   3. Every adapter, transport and the shared tool crate has a normal dependency on core/.
//   5. Test doubles and contract suites stay out of every product build (#57, #59): no
//      product member (core, cli, an adapter or a transport) reaches a tests/fakes/<name>
//      or tests/protocol/contract/<name> crate over normal and build edges alone, so none
//      is compiled into the `oac` binary. An adapter, a transport or cli/ may take one as a
//      dev-dependency (rule 2 allows the reach); core/ may not, because core/ reaches
//      nothing in-repo. The security suite (#60) is a test crate itself, so it may build a
//      fake and a transport in; no member at all may reach it (rule 2).
//   4. Transport crates stay with their owner: the zenoh crates may be a direct dependency
//      of transports/zenoh only (07 section 5, "Consuming module"), and may not be
//      reachable from any member other than their owner and cli/. So none is reachable from
//      core/. The device-key storage crates (keyring and its backends, age) are owned by
//      cli/ the same way (#52, #315 review N-b).
//   6. Forbidden crates are reachable from no member at all, cli/ included, over any edge
//      kind (#7; ADR-001 Boundary; docs/planning/decisions/G-7-stage4-dependencies.md
//      section 2): every `codex-*` crate (FORBIDDEN_FAMILIES; no Codex crate is allowed),
//      and `rmcp-macros` (FORBIDDEN_NAMES; the vetted rmcp form refuses its `macros`
//      feature, and a crate elsewhere in the graph that turned it on would unify it into
//      the adapters' build, PR #352 review finding 2). A forbidden name fails wherever it
//      appears in a member's closure, so a crate that only reaches one fails too.
//      Names match case-insensitively, with `_` folded to `-`. Limit: a vendored copy whose
//      [package] name is edited is outside any check by name (G-7 section 2.3).
//
//   --adapters-alone runs `cargo check --locked --lib` on the adapters and adapters/mcp-tools
//   only, so their dependencies' features unify among themselves and not with cli/'s (PR
//   #352 review finding 2): a feature only another member turns on (tokio's `net`, say) is
//   then absent, and an adapter that uses it fails to build. CI runs it in job crate-deps.
//
//   node scripts/check-crate-deps.mjs                    # check this workspace
//   node scripts/check-crate-deps.mjs --metadata <file>  # check a saved metadata JSON
//   node scripts/check-crate-deps.mjs --self-test        # synthetic graphs, each planted
//                                                        # violation must fail
//   node scripts/check-crate-deps.mjs --adapters-alone   # cargo check the adapters alone
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
  {
    family: 'OS credential store',
    re: /^(?:keyring(?:-core)?|[a-z0-9-]+-keyring-store|secret-service|security-framework(?:-sys)?)$/,
    owner: 'cli',
  },
  { family: 'encrypted-file key store', re: /^age(?:-core)?$/, owner: 'cli' },
];
const fold = (name) => String(name).toLowerCase().replace(/_/g, '-');
export const ownedFamily = (name) => OWNED_EXTERNAL.find((o) => o.re.test(fold(name)));

// Rule 6 (#7; G-7 section 2): crates no member may reach. The lead's decision is that no
// Codex crate is allowed, so the whole `codex-` family is refused (PR #352 review finding
// 4), not a list. `rmcp-macros` is refused by name (finding 2). The same two lists are
// FORBIDDEN_FAMILIES and FORBIDDEN_NAMES in tests/protocol/contract/adapter/src/source.rs;
// a test there checks they agree.
export const FORBIDDEN_FAMILIES = [
  'codex-',
];
export const FORBIDDEN_NAMES = [
  'rmcp-macros',
];
export const isForbidden = (name) => {
  const f = fold(name);
  return FORBIDDEN_FAMILIES.some((x) => f.startsWith(x)) || FORBIDDEN_NAMES.includes(f);
};
// The chains recorded at the Codex repository's tag rust-v0.161.0 (G-7 section 2.2), kept as
// self-test evidence: each is, or reaches, a model API client (codex-api calls the
// provider's /responses endpoint), a keyring store (codex-keyring-store) or the rollout
// files (codex-rollout):
// codex-app-server-protocol -> codex-rollout -> codex-otel -> codex-api -> codex-client;
// codex-app-server-protocol -> codex-secrets -> codex-keyring-store;
// codex-app-server-transport -> codex-core, codex-login, codex-api, codex-model-provider;
// codex-app-server-client -> codex-app-server, codex-core;
// codex-protocol -> codex-network-proxy, codex-http-client.
// Plus crates that reach none of those (review finding 4): codex-responses-api-proxy (a
// model-API proxy), codex-websocket-auth, and the two proc-macro crates.
const RECORDED_CODEX_CRATES = [
  'codex-app-server', 'codex-app-server-client', 'codex-app-server-protocol', 'codex-app-server-transport',
  'codex-core', 'codex-api', 'codex-client', 'codex-login', 'codex-keyring-store', 'codex-secrets',
  'codex-rollout', 'codex-state', 'codex-model-provider', 'codex-otel', 'codex-http-client',
  'codex-network-proxy', 'codex-protocol', 'codex-responses-api-proxy', 'codex-websocket-auth',
  'codex-app-server-protocol-noop-macros', 'codex-experimental-api-macros', 'codex-utils-string',
];

// Every cargo metadata call resolves with --all-features: an optional dependency behind a
// non-default feature is still a dependency the build can compile in, so it must be in
// the graph both checks read (#311 review B1).
export const METADATA_ARGS = ['metadata', '--format-version', '1', '--offline', '--all-features'];

// Module of a workspace member, from its manifest directory relative to the workspace root.
export function moduleOf(relDir) {
  const d = relDir.split(sep).join('/');
  if (d === 'core') return { kind: 'core', path: d };
  if (d === 'cli') return { kind: 'cli', path: d };
  // The MCP tool surface both adapters share (#7, G-7 section 4): its own kind, matched
  // before the adapter pattern, so it is never an adapter itself.
  if (d === 'adapters/mcp-tools') return { kind: 'tools', path: d };
  let m = /^adapters\/([a-z0-9][a-z0-9_-]*)$/.exec(d);
  if (m) return { kind: 'adapter', path: d };
  m = /^transports\/([a-z0-9][a-z0-9_-]*)$/.exec(d);
  if (m) return { kind: 'transport', path: d };
  m = /^tests\/fakes\/([a-z0-9][a-z0-9_-]*)$/.exec(d);
  if (m) return { kind: 'fake', path: d };
  m = /^tests\/protocol\/contract\/([a-z0-9][a-z0-9_-]*)$/.exec(d);
  if (m) return { kind: 'suite', path: d };
  if (d === 'tests/security') return { kind: 'security', path: d };
  return null;
}

// May a member of kind `from` reach workspace member `to` (by module)?
// Over every edge kind; rule 5 separately keeps fakes off normal and build edges.
function allowedReach(from, to) {
  if (to.kind === 'cli') return false;
  if (to.kind === 'security') return false; // the security suite is a leaf (#60)
  if (from.kind === 'cli') return true;
  if (from.kind === 'core') return false;
  if (from.kind === 'fake') return to.kind === 'core'; // test doubles: core/ only
  // The shared tool crate (#7): core/, and a fake or a suite as a dev-dependency (rule 5).
  if (from.kind === 'tools') return to.kind === 'core' || to.kind === 'fake' || to.kind === 'suite';
  if (from.kind === 'suite') return to.kind === 'core' || to.kind === 'fake'; // contract suites
  // The security suite (#60): core/, a fake, a transport to carry envelopes over, and a
  // contract suite, which a transport reaches as a dev-dependency (#59).
  if (from.kind === 'security') return ['core', 'fake', 'transport', 'suite'].includes(to.kind);
  // adapters: also the shared tool crate (#7)
  if (from.kind === 'adapter' && to.kind === 'tools') return true;
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
          '(core, cli, adapters/<name>, adapters/mcp-tools, transports/<name>, tests/fakes/<name>, tests/protocol/contract/<name>, tests/security)',
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
    // 3. adapters, transports and the shared tool crate depend on core/ (normal edge).
    if (mod.kind === 'adapter' || mod.kind === 'transport' || mod.kind === 'tools') {
      const hasCore = depsOf(id).some(
        (d) => members.get(d.id)?.kind === 'core' && d.kinds.some((k) => k.kind === null || k.kind === 'normal'),
      );
      if (!hasCore) violations.push(`${name}: no normal dependency on core/ (adapters, transports and adapters/mcp-tools depend on core)`);
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
      // 6. forbidden crates (#7): no member may reach one, cli/ included.
      if (isForbidden(pname)) {
        violations.push(`${name} reaches ${pname} (forbidden, rule 6: G-7 section 2): ${pathTo(reached)}`);
        continue;
      }
      const o = ownedFamily(pname);
      if (o && mod.path !== o.owner && mod.kind !== 'cli') {
        violations.push(`${name} reaches ${pname} (${o.family}, owned by ${o.owner}): ${pathTo(reached)}`);
      }
    }
    // 5. No test double in a product build: reachability over normal and build edges only.
    if (mod.kind !== 'fake' && mod.kind !== 'suite' && mod.kind !== 'security') {
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
const TOOLS_MEMBERS = { ...BASE_MEMBERS, 'oac-mcp-tools': 'adapters/mcp-tools' };
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
const SECURITY_MEMBERS = { ...SUITE_MEMBERS, 'oac-security-suite': 'tests/security' };
const SECURITY_EDGES = [
  ...SUITE_EDGES,
  ['oac-security-suite', 'oac-core'],
  ['oac-security-suite', 'oac-fake-claude'],
  ['oac-security-suite', 'oac-transport-memory'],
  ['oac-transport-memory', 'oac-contract-transport', 'dev'],
];
const without = (edges, f, t) => edges.filter(([a, b]) => !(a === f && b === t));
const TOOLS_EDGES = [
  ...BASE_EDGES,
  ['oac-mcp-tools', 'oac-core'],
  ['oac-adapter-claude', 'oac-mcp-tools'],
  ['oac-adapter-codex', 'oac-mcp-tools'],
  ['oac-cli', 'oac-mcp-tools'],
];

// #7 rule 6: every recorded Codex crate is refused as a direct dependency of adapters/codex,
// of core/ and of cli/ (the old rule-4 family refused codex-app-server-* in core/ and cli/;
// review finding 4).
const FORBIDDEN_DIRECT_CASES = RECORDED_CODEX_CRATES.flatMap((n) =>
  [['oac-adapter-codex', 'adapters/codex'], ['oac-core', 'core'], ['oac-cli', 'cli']].map(([m, d]) => ({
    name: `${d} depends on ${n} (forbidden)`,
    meta: synth({ members: BASE_MEMBERS, externals: [n], edges: [...BASE_EDGES, [m, n]] }),
  })),
);

const SELF_TEST_CASES = [
  { name: 'control: the scaffold graph is clean', expectClean: true, meta: synth({ members: BASE_MEMBERS, edges: BASE_EDGES }) },
  {
    name: 'control: owners may depend on their own external crates; cli may reach them',
    expectClean: true,
    meta: synth({
      members: BASE_MEMBERS,
      externals: ['zenoh', 'zenoh-protocol', 'serde'],
      edges: [...BASE_EDGES, ['oac-transport-zenoh', 'zenoh'], ['zenoh', 'zenoh-protocol'], ['oac-core', 'serde']],
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
  // #7 (G-7 section 2): no Codex crate is allowed, not even in adapters/codex.
  { name: 'adapters/codex depends on codex_app_server_protocol (forbidden)', meta: synth({ members: BASE_MEMBERS, externals: ['codex_app_server_protocol'], edges: [...BASE_EDGES, ['oac-adapter-codex', 'codex_app_server_protocol']] }) },
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
  // #60: the security suite at tests/security reaches core/, a fake and a transport, and
  // nothing reaches it.
  {
    name: 'control: the security suite depends on core/, a fake and transports/memory',
    expectClean: true,
    meta: synth({ members: SECURITY_MEMBERS, edges: SECURITY_EDGES }),
  },
  { name: 'security suite -> adapter', meta: synth({ members: SECURITY_MEMBERS, edges: [...SECURITY_EDGES, ['oac-security-suite', 'oac-adapter-claude']] }) },
  { name: 'security suite -> cli (dev-dependency)', meta: synth({ members: SECURITY_MEMBERS, edges: [...SECURITY_EDGES, ['oac-security-suite', 'oac-cli', 'dev']] }) },
  { name: 'control: the security suite may reach a contract suite', expectClean: true, meta: synth({ members: SECURITY_MEMBERS, edges: [...SECURITY_EDGES, ['oac-security-suite', 'oac-contract-transport']] }) },
  {
    name: 'security suite reaches zenoh through transports/zenoh',
    meta: synth({
      members: SECURITY_MEMBERS,
      externals: ['zenoh'],
      edges: [...SECURITY_EDGES, ['oac-transport-zenoh', 'zenoh'], ['oac-security-suite', 'oac-transport-zenoh']],
    }),
  },
  { name: 'transport -> security suite (dev-dependency)', meta: synth({ members: SECURITY_MEMBERS, edges: [...SECURITY_EDGES, ['oac-transport-memory', 'oac-security-suite', 'dev']] }) },
  { name: 'cli -> security suite (dev-dependency)', meta: synth({ members: SECURITY_MEMBERS, edges: [...SECURITY_EDGES, ['oac-cli', 'oac-security-suite', 'dev']] }) },
  { name: 'core -> security suite (dev-dependency)', meta: synth({ members: SECURITY_MEMBERS, edges: [...SECURITY_EDGES, ['oac-core', 'oac-security-suite', 'dev']] }) },
  {
    name: 'security suite outside tests/security (tests/security/inner)',
    meta: synth({ members: { ...SECURITY_MEMBERS, 'oac-security-suite': 'tests/security/inner' }, edges: SECURITY_EDGES }),
  },
  // #7 rule 6: forbidden Codex crates, by name and by transitive presence (G-7 section 2).
  ...FORBIDDEN_DIRECT_CASES,
  {
    name: 'cli/ reaches codex-core through a third-party crate (forbidden even for cli/)',
    meta: synth({ members: BASE_MEMBERS, externals: ['helper', 'codex-core'], edges: [...BASE_EDGES, ['oac-cli', 'helper'], ['helper', 'codex-core']] }),
  },
  {
    name: 'adapters/codex reaches codex-api through codex-rollout and codex-otel (the recorded chain)',
    meta: synth({
      members: BASE_MEMBERS,
      externals: ['codex-app-server-protocol', 'codex-rollout', 'codex-otel', 'codex-api'],
      edges: [...BASE_EDGES, ['oac-adapter-codex', 'codex-app-server-protocol'], ['codex-app-server-protocol', 'codex-rollout'],
        ['codex-rollout', 'codex-otel'], ['codex-otel', 'codex-api']],
    }),
  },
  {
    name: 'an unlisted crate that only reaches codex-keyring-store fails transitively',
    meta: synth({ members: BASE_MEMBERS, externals: ['innocent-types', 'codex-keyring-store'], edges: [...BASE_EDGES, ['oac-adapter-claude', 'innocent-types'], ['innocent-types', 'codex-keyring-store']] }),
  },
  { name: 'adapters/codex dev-depends on codex-rollout (forbidden on dev edges too)', meta: synth({ members: BASE_MEMBERS, externals: ['codex-rollout'], edges: [...BASE_EDGES, ['oac-adapter-codex', 'codex-rollout', 'dev']] }) },
  { name: 'transports/zenoh build-depends on codex-login', meta: synth({ members: BASE_MEMBERS, externals: ['codex-login'], edges: [...BASE_EDGES, ['oac-transport-zenoh', 'codex-login', 'build']] }) },
  { name: 'core depends on Codex_Core (mixed case, underscore)', meta: synth({ members: BASE_MEMBERS, externals: ['Codex_Core'], edges: [...BASE_EDGES, ['oac-core', 'Codex_Core']] }) },
  // Review finding 4: a codex-* crate outside every recorded chain fails by family.
  { name: 'adapters/claude depends on codex-anything-new (family match)', meta: synth({ members: BASE_MEMBERS, externals: ['codex-anything-new'], edges: [...BASE_EDGES, ['oac-adapter-claude', 'codex-anything-new']] }) },
  { name: 'transports/zenoh reaches CODEX_utils_cache through a helper (family, folded)', meta: synth({ members: BASE_MEMBERS, externals: ['helper', 'CODEX_utils_cache'], edges: [...BASE_EDGES, ['oac-transport-zenoh', 'helper'], ['helper', 'CODEX_utils_cache']] }) },
  {
    name: 'control: names that only contain "codex" are not the family',
    expectClean: true,
    meta: synth({ members: BASE_MEMBERS, externals: ['mycodex', 'codexx', 'rmcp'], edges: [...BASE_EDGES, ['oac-adapter-codex', 'mycodex'], ['oac-adapter-codex', 'codexx'], ['oac-adapter-codex', 'rmcp']] }),
  },
  // Review finding 2: rmcp-macros anywhere in the graph, cli/ included.
  { name: 'cli/ reaches rmcp-macros through rmcp (forbidden anywhere)', meta: synth({ members: BASE_MEMBERS, externals: ['rmcp', 'rmcp-macros'], edges: [...BASE_EDGES, ['oac-cli', 'rmcp'], ['rmcp', 'rmcp-macros']] }) },
  { name: 'adapters/claude depends on rmcp_macros (underscore)', meta: synth({ members: BASE_MEMBERS, externals: ['rmcp_macros'], edges: [...BASE_EDGES, ['oac-adapter-claude', 'rmcp_macros']] }) },
  // #7: the shared MCP tool crate at adapters/mcp-tools (G-7 section 4).
  { name: 'control: both adapters and cli/ depend on adapters/mcp-tools, which depends on core/', expectClean: true, meta: synth({ members: TOOLS_MEMBERS, edges: TOOLS_EDGES }) },
  {
    name: 'control: adapters/mcp-tools may take a fake as a dev-dependency',
    expectClean: true,
    meta: synth({ members: { ...TOOLS_MEMBERS, 'oac-fake-claude': 'tests/fakes/claude' }, edges: [...TOOLS_EDGES, ['oac-fake-claude', 'oac-core'], ['oac-mcp-tools', 'oac-fake-claude', 'dev']] }),
  },
  { name: 'adapters/mcp-tools -> adapter', meta: synth({ members: TOOLS_MEMBERS, edges: [...TOOLS_EDGES, ['oac-mcp-tools', 'oac-adapter-claude']] }) },
  { name: 'adapters/mcp-tools -> transport', meta: synth({ members: TOOLS_MEMBERS, edges: [...TOOLS_EDGES, ['oac-mcp-tools', 'oac-transport-zenoh']] }) },
  { name: 'adapters/mcp-tools -> cli (dev-dependency)', meta: synth({ members: TOOLS_MEMBERS, edges: [...TOOLS_EDGES, ['oac-mcp-tools', 'oac-cli', 'dev']] }) },
  { name: 'adapters/mcp-tools without a core/ dependency', meta: synth({ members: TOOLS_MEMBERS, edges: without(TOOLS_EDGES, 'oac-mcp-tools', 'oac-core') }) },
  { name: 'core -> adapters/mcp-tools', meta: synth({ members: TOOLS_MEMBERS, edges: [...TOOLS_EDGES, ['oac-core', 'oac-mcp-tools']] }) },
  { name: 'transport -> adapters/mcp-tools', meta: synth({ members: TOOLS_MEMBERS, edges: [...TOOLS_EDGES, ['oac-transport-zenoh', 'oac-mcp-tools']] }) },
  {
    name: 'a fake -> adapters/mcp-tools',
    meta: synth({ members: { ...TOOLS_MEMBERS, 'oac-fake-claude': 'tests/fakes/claude' }, edges: [...TOOLS_EDGES, ['oac-fake-claude', 'oac-core'], ['oac-fake-claude', 'oac-mcp-tools']] }),
  },
  { name: 'adapters/mcp-tools depends on zenoh', meta: synth({ members: TOOLS_MEMBERS, externals: ['zenoh'], edges: [...TOOLS_EDGES, ['oac-mcp-tools', 'zenoh']] }) },
  { name: 'adapters/mcp-tools depends on codex-app-server-protocol', meta: synth({ members: TOOLS_MEMBERS, externals: ['codex-app-server-protocol'], edges: [...TOOLS_EDGES, ['oac-mcp-tools', 'codex-app-server-protocol']] }) },
  { name: 'nested tool crate path (adapters/mcp-tools/inner)', meta: synth({ members: { ...TOOLS_MEMBERS, 'oac-inner': 'adapters/mcp-tools/inner' }, edges: [...TOOLS_EDGES, ['oac-inner', 'oac-core']] }) },
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
  'codex-api': { name: 'codex-api', license: 'Apache-2.0' },
  'codex-responses-api-proxy': { name: 'codex-responses-api-proxy', license: 'Apache-2.0' },
  'rmcp-macros': { name: 'rmcp-macros', license: 'Apache-2.0' },
  // A crate whose `net` module exists only under its `net` feature (review finding 2): an
  // adapter that uses it builds in a workspace build when cli/ turns `net` on, and fails
  // under --adapters-alone.
  'featured-stub': {
    name: 'featured-stub',
    license: 'MIT',
    features: '[features]\nnet = []\n',
    lib: '#[cfg(feature = "net")]\npub mod net {\n    pub struct Probe;\n}\n',
  },
  // A crate under an innocent name that reaches a forbidden one (#7 rule 6).
  'types-helper': { name: 'types-helper', license: 'Apache-2.0', deps: ['codex-api = { path = "../codex-api" }'] },
  'zenoh-backend-traits': { name: 'zenoh_backend_traits', license: 'EPL-2.0 OR Apache-2.0' },
  'codex-app-server-protocol': { name: 'codex_app_server_protocol', license: 'Apache-2.0' },
  'gpl-stub': { name: 'gpl-stub', license: 'GPL-3.0-only' },
  // A uniquely named EPL/Apache dual, so a license control never collides with a real
  // `zenoh` key once transports/zenoh depends on zenoh (G1, #62).
  'epl-dual-stub': { name: 'epl-dual-stub', license: 'EPL-2.0 OR Apache-2.0' },
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
        `[package]\nname = "${s.name}"\nversion = "0.0.1"\nedition = "2024"\nlicense = "${s.license}"\n` +
          (s.deps ? `\n[dependencies]\n${s.deps.join('\n')}\n` : '') + (s.features ? `\n${s.features}` : ''));
      writeFileSync(join(tmp, 'stubs', dir, 'src', 'lib.rs'), s.lib ?? '');
    }
    for (const e of edit ? (edit.edits ?? [edit]) : []) {
      const p = join(ws, e.file);
      const before = readFileSync(p, 'utf8').replace(/\r\n/g, '\n');
      const after = e.edit(before);
      if (after === before) throw new Error(`mutation "${edit.name}" did not change ${e.file}`);
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
  // #60: the security suite is a leaf, and reaches no adapter.
  { name: 'cli/ dev-depends on the security suite', file: 'cli/Cargo.toml', edit: addDep('dev-dependencies', 'oac-security-suite = { path = "../tests/security" }') },
  { name: 'transports/memory dev-depends on the security suite', file: 'transports/memory/Cargo.toml', edit: addDep('dev-dependencies', 'oac-security-suite = { path = "../../tests/security" }') },
  { name: 'the security suite depends on adapters/claude', file: 'tests/security/Cargo.toml', edit: addDep('dependencies', 'oac-adapter-claude = { path = "../../adapters/claude" }') },
  // #7 rule 6: forbidden Codex crates, directly and through an innocent-looking crate.
  { name: 'adapters/codex depends on codex_app_server_protocol (forbidden)', file: 'adapters/codex/Cargo.toml', edit: addDep('dependencies', 'codex_app_server_protocol = { path = "../../../stubs/codex-app-server-protocol" }') },
  { name: 'cli/ depends on a crate that reaches codex-api (forbidden transitively)', file: 'cli/Cargo.toml', edit: addDep('dependencies', 'types-helper = { path = "../../stubs/types-helper" }') },
  { name: 'adapters/codex dev-depends on codex-api (forbidden on dev edges)', file: 'adapters/codex/Cargo.toml', edit: addDep('dev-dependencies', 'codex-api = { path = "../../../stubs/codex-api" }') },
  // Review finding 4: family match, and the old rule-4 family's core/ and cli/ refusals.
  { name: 'core/ depends on codex-responses-api-proxy (family match)', file: 'core/Cargo.toml', edit: addDep('dependencies', 'codex-responses-api-proxy = { path = "../../stubs/codex-responses-api-proxy" }') },
  { name: 'cli/ depends on codex_app_server_protocol (was rule 4, now rule 6)', file: 'cli/Cargo.toml', edit: addDep('dependencies', 'codex_app_server_protocol = { path = "../../stubs/codex-app-server-protocol" }') },
  // Review finding 2: rmcp-macros anywhere.
  { name: 'cli/ depends on rmcp-macros (forbidden anywhere)', file: 'cli/Cargo.toml', edit: addDep('dependencies', 'rmcp-macros = { path = "../../stubs/rmcp-macros" }') },
  // #7: the shared MCP tool crate.
  { name: 'adapters/mcp-tools depends on adapters/claude', file: 'adapters/mcp-tools/Cargo.toml', edit: addDep('dependencies', 'oac-adapter-claude = { path = "../claude" }') },
  { name: 'transports/memory depends on adapters/mcp-tools', file: 'transports/memory/Cargo.toml', edit: addDep('dependencies', 'oac-mcp-tools = { path = "../../adapters/mcp-tools" }') },
];

// Review finding 2 (feature unification): another member turns a feature on that an
// adapter's own dependency line leaves off, and the adapter uses it. Building the two
// together (as every workspace build, and the `oac` binary through cli/, does) compiles;
// the adapters-alone check must not.
const UNIFICATION_MUTATION = {
  name: 'adapters/codex uses a feature only another member turns on (caught by --adapters-alone, not by a build with that member)',
  edits: [
    { file: 'adapters/codex/Cargo.toml', edit: addDep('dependencies', 'featured-stub = { path = "../../../stubs/featured-stub" }') },
    { file: 'transports/memory/Cargo.toml', edit: addDep('dependencies', 'featured-stub = { path = "../../../stubs/featured-stub", features = ["net"] }') },
    { file: 'adapters/codex/src/lib.rs', edit: (t) => `${t}\npub type Probe = featured_stub::net::Probe;\n` },
  ],
};

// The adapter-side members: adapters/<name> and adapters/mcp-tools.
export function adapterPackages(meta) {
  const root = meta.workspace_root;
  return meta.packages
    .filter((p) => meta.workspace_members.includes(p.id))
    .filter((p) => ['adapter', 'tools'].includes(moduleOf(relative(root, dirname(p.manifest_path)))?.kind))
    .map((p) => p.name)
    .sort();
}

// `cargo check --lib` on the adapter-side members alone (review finding 2).
export function adaptersAlone(dir) {
  const m = cargoMetadata(dir, ['--no-deps']);
  if (m.status !== 0) return { status: 2, out: m.stderr };
  const names = adapterPackages(JSON.parse(m.stdout));
  if (names.length === 0) return { status: 2, out: 'no adapter packages found' };
  const r = spawnSync('cargo', ['check', '--locked', '--offline', '--lib', ...names.flatMap((n) => ['-p', n])], {
    cwd: dir,
    encoding: 'utf8',
    maxBuffer: 256 * 1024 * 1024,
  });
  if (r.error) throw r.error;
  return { status: r.status, out: `${r.stdout}${r.stderr}`, names };
}

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
  // Review finding 2: the unification case. Not --locked in the copy (the stub is new), so
  // each run uses its own lockfile resolution, offline.
  {
    const r = withWorkspaceCopy(UNIFICATION_MUTATION, (ws) => {
      const unlock = (args) => spawnSync('cargo', args, { cwd: ws, encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 });
      const gen = unlock(['generate-lockfile', '--offline']);
      if (gen.status !== 0) return { malformed: `generate-lockfile: ${gen.stderr.trim().split('\n').pop()}` };
      // Another member and the adapter together: the unification every workspace build, and
      // the `oac` binary through cli/, gets. transports/memory stands in for cli/ because it
      // builds in seconds (it depends on core/ only).
      const whole = unlock(['check', '--offline', '--lib', '-p', 'oac-transport-memory', '-p', 'oac-adapter-codex']);
      const alone = adaptersAlone(ws);
      return { whole: whole.status, alone: alone.status, aloneOut: alone.out };
    });
    const ok = !r.malformed && r.whole === 0 && r.alone !== 0 && /E0433|E0412|E0425|could not find `net`|cannot find/.test(r.aloneOut ?? '');
    if (!ok) bad++;
    console.log(`${ok ? 'pass' : 'FAIL'}  ${UNIFICATION_MUTATION.name} -- ${r.malformed ?? `member+adapter build exit ${r.whole}, adapters-alone exit ${r.alone}`}`);
  }
  const total = cases.length + 1;
  console.log(`mutation test: ${total - bad}/${total} cases pass`);
  return bad ? 1 : 0;
}

// ---------------------------------------------------------------------------------------

function main(argv) {
  if (argv[0] === '--self-test') return selfTest();
  if (argv[0] === '--mutation-test') return mutationTest();
  if (argv[0] === '--adapters-alone') {
    const r = adaptersAlone(repoRoot);
    if (r.status === 0) {
      console.log(`adapters alone: CLEAN (cargo check --lib -p ${r.names.join(' -p ')})`);
      return 0;
    }
    console.log(r.out);
    console.log('adapters alone: FAIL -- an adapter needs a dependency feature it does not turn on itself (G-7 section 5)');
    return r.status === 2 ? 2 : 1;
  }
  if (argv[0] === '--metadata') {
    if (!argv[1]) {
      console.error('usage: --metadata <file>');
      return 2;
    }
    return report(checkMetadata(JSON.parse(readFileSync(argv[1], 'utf8'))), argv[1]);
  }
  if (argv.length) {
    console.error('usage: check-crate-deps.mjs [--self-test | --mutation-test | --adapters-alone | --metadata <file>]');
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

