#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
//
// Client-side CI. There is no GitHub-hosted CI: the lead decided on 2026-10-08 that PR checks
// run client-side ("Tests for PR should be done client side"), then that "Anything that has an
// associated cost on the github side needs to go". Every check the deleted workflows ran
// (ci.yml, boundary-lint.yml, keystore-optin.yml, scale-optin.yml,
// security-mutation-optin.yml, g3-macos-hosted.yml) is a step here, defined in this file
// (and, for the bash bodies, in .github/local-ci.sh). Run it on Windows and in WSL/Linux
// and paste each summary into the PR.
//
//   node scripts/local-ci.mjs                  # the default tier: every PR and merge check
//   node scripts/local-ci.mjs --quick          # the same, minus the herdr self-test's slow
//                                              # lifecycle half (POSIX only); not for a PR
//   node scripts/local-ci.mjs --tier <name>    # an opt-in tier, on demand:
//        keystore    real OS credential stores (#52; was keystore-optin.yml)
//        scale       full-scale bound tests, release build (#328; was scale-optin.yml)
//        mutation    security-suite mutation check (#329; was security-mutation-optin.yml)
//        g3-macos    the G3 Zenoh peer Mac leg (#219; was g3-macos-hosted.yml), macOS only
//   node scripts/local-ci.mjs --list [--tier <name>]   # print the steps, run nothing
//   node scripts/local-ci.mjs --only <id>[,<id>...]    # run only these steps (the summary
//                                                      # says PARTIAL; not for a PR)
//   node scripts/local-ci.mjs --self-test      # prove every step still runs (see below)
//
// The default tier keeps the old rules (oac-testing section 2; PLANNING-PROMPT sections 6 and
// 9.10): no live provider, no API key, no network beyond loopback. The one networked step is
// `cargo fetch --locked`; every cargo step after it runs with CARGO_NET_OFFLINE=true. On Linux
// (WSL included) the test steps run inside scripts/loopback-only.sh, after its --select and
// --probe steps, exactly as ci.yml ran them on the ubuntu image; elsewhere the rule rests on
// the offline cargo setting and the fakes binding loopback only, as on ci.yml's other images.
// CARGO_INCREMENTAL=0 as in ci.yml.
//
// The summary at the end names the git HEAD SHA, says whether the work tree was clean (the
// index-scanning checks read the index, the cargo steps the work tree), and lists every step
// as PASS, FAIL, SKIP (with the reason) or NOT RUN (a missing prerequisite). Paste it into
// the PR as it is.
//
// --self-test (part of the default tier, so every summary carries it):
//   - coverage: every step of the deleted workflows, listed in PORTED below by its old job
//     and step, maps to a step here, so a step cannot drop out unnoticed;
//   - plan: the plan is built for linux, win32 and darwin, full and --quick, and run against
//     a recording stub: every step is attempted, the sandboxed ones inside the loopback
//     wrapper on Linux, and --quick changes nothing but the herdr self-test's environment;
//   - commands: every script a step runs exists, and every flag it passes is in that script;
//   - policy (moved here from scripts/check-workflows.mjs D2 and D3 with the hosted default
//     tier): no default-tier step runs an ignored test, sets an OAC_TEST_* opt-in, reaches
//     the herdr driver beyond its exact offline self-test, or installs a harness CLI; every
//     cargo step after the fetch is offline; planted violations must be caught;
//   - boundary lint: each `rg` check of .github/local-ci.sh passes a clean throwaway git tree
//     and fails its planted violation.
//
// Node built-ins only. Exit codes: 0 = every step passed (SKIPs included); 1 = a step failed
// (or the self-test did); 2 = usage or environment error; 3 = no failure, but a step was
// NOT RUN (a missing prerequisite, or a tier this OS cannot run).

import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir, release as osRelease } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptPath = fileURLToPath(import.meta.url);
const repoRoot = resolve(dirname(scriptPath), '..');
const BASH_BODIES = '.github/local-ci.sh';
const TOOLCHAIN = '1.98.1'; // rust-toolchain.toml; docs/planning/PINS.md

// ---- steps ----------------------------------------------------------------------------
//
// A step: { id, name, cmd: [program, ...args] | bash: <section of .github/local-ci.sh> |
// check: <built-in check name>, env, sandbox (wrap in loopback-only.sh on Linux), offline
// (CARGO_NET_OFFLINE=true), os (only on these platforms; elsewhere SKIP with osReason),
// needs (programs that must be on PATH, else NOT RUN), quickEnv (env added under --quick),
// paths (set from PATHS below: what the step depends on) }.
// `node` means this Node; `bash` means Git Bash on Windows.

const OFFLINE = { CARGO_NET_OFFLINE: 'true' };
const LINUX_ONLY_SANDBOX = 'not Linux: no network namespace; the rule rests on offline cargo and loopback-only fakes, as on the old windows/macos CI legs';

const toolchain = { id: 'toolchain', name: `rustc is the ${TOOLCHAIN} pin (rust-toolchain.toml)`, check: 'toolchain' };
const fetch = { id: 'cargo-fetch', name: 'cargo fetch --locked (the one networked step)', cmd: ['cargo', 'fetch', '--locked'] };
const loopbackSelect = { id: 'loopback-select', name: 'loopback-only sandbox: select (mandatory on Linux)', check: 'loopback-select', os: ['linux'], osReason: LINUX_ONLY_SANDBOX };
const loopbackProbe = { id: 'loopback-probe', name: 'loopback-only sandbox: probe (loopback connects, nothing else is reachable)', cmd: ['bash', 'scripts/loopback-only.sh', '--probe'], os: ['linux'], osReason: LINUX_ONLY_SANDBOX };

export const TIERS = {
  default: [
    // ci.yml job `test`
    toolchain,
    { id: 'fmt', name: 'cargo fmt --check', cmd: ['cargo', 'fmt', '--all', '--check'] },
    fetch,
    { id: 'build', name: 'cargo build (all targets)', cmd: ['cargo', 'build', '--workspace', '--all-targets', '--locked'], offline: true },
    { id: 'clippy', name: 'cargo clippy -D warnings', cmd: ['cargo', 'clippy', '--workspace', '--all-targets', '--locked', '--', '-D', 'warnings'], offline: true },
    loopbackSelect,
    loopbackProbe,
    { id: 'cargo-test', name: 'unit, contract, security, fake-integration and conformance tiers (cargo test)', cmd: ['cargo', 'test', '--workspace', '--locked'], offline: true, sandbox: true },
    // ci.yml job `crate-deps`
    { id: 'crate-deps-self-test', name: 'dependency direction self-test', cmd: ['node', 'scripts/check-crate-deps.mjs', '--self-test'] },
    { id: 'crate-deps', name: 'dependency direction', cmd: ['node', 'scripts/check-crate-deps.mjs'], offline: true },
    { id: 'crate-deps-mutation', name: 'dependency direction mutation test', cmd: ['node', 'scripts/check-crate-deps.mjs', '--mutation-test'], offline: true },
    { id: 'crate-deps-adapters-alone', name: 'adapters build alone (no feature borrowed from another member; #7, G-7 section 5)', cmd: ['node', 'scripts/check-crate-deps.mjs', '--adapters-alone'], offline: true },
    // ci.yml job `licenses`
    { id: 'licenses-self-test', name: 'license check self-test', cmd: ['node', 'scripts/check-licenses.mjs', '--self-test'] },
    { id: 'licenses-mutation', name: 'license check mutation test', cmd: ['node', 'scripts/check-licenses.mjs', '--mutation-test'], offline: true },
    { id: 'licenses', name: 'license headers and dependency inventory', cmd: ['node', 'scripts/check-licenses.mjs'], offline: true },
    // boundary-lint.yml jobs `containment`, `fixture-manifest`, `workflow-policy`
    { id: 'containment-self-test', name: 'checks 12-13 self-test', cmd: ['node', 'scripts/check-containment.mjs', '--self-test'] },
    { id: 'containment', name: 'check 12 Zenoh containment, check 13 test-double containment', cmd: ['node', 'scripts/check-containment.mjs'] },
    { id: 'fixture-manifest-self-test', name: 'fixture manifest self-test', cmd: ['node', 'scripts/check-fixture-manifest.mjs', '--self-test'] },
    { id: 'fixture-manifest', name: 'fixture manifest (MANIFEST.json matches committed fixtures; needs full history)', cmd: ['node', 'scripts/check-fixture-manifest.mjs'] },
    { id: 'workflows-self-test', name: 'workflow policy self-test', cmd: ['node', 'scripts/check-workflows.mjs', '--self-test'] },
    { id: 'workflows', name: 'workflow policy (W6 no cargo source override, W7 dispatch only, W8 self-hosted only)', cmd: ['node', 'scripts/check-workflows.mjs'] },
    // boundary-lint.yml job `boundary-lint`, checks 9-10
    { id: 'herdr-containment-self-test', name: 'checks 9-10 self-test', cmd: ['node', 'scripts/check-herdr-containment.mjs', '--self-test'] },
    { id: 'herdr-containment', name: 'checks 9-10 herdr containment', cmd: ['node', 'scripts/check-herdr-containment.mjs'] },
    // boundary-lint.yml job `agents-skills-sync`
    { id: 'skills', name: 'skill budgets', cmd: ['node', 'scripts/check-skills.mjs'] },
    { id: 'agents-skills-sync-self-test', name: 'Codex skills copy self-test', cmd: ['node', 'scripts/sync-agents-skills.mjs', '--self-test'] },
    { id: 'agents-skills-sync', name: 'Codex skills copy matches its source', cmd: ['node', 'scripts/sync-agents-skills.mjs', '--check'] },
    // ci.yml job `herdr-selftest`
    {
      id: 'herdr-self-test',
      name: 'herdr driver self-test (test doubles only; lifecycle half on POSIX)',
      cmd: ['node', 'tools/herdr/run.mjs', '--self-test'],
      sandbox: true,
      quickEnv: { OAC_HERDR_SELFTEST_UNIT_ONLY: '1' },
    },
    // ci.yml job `test`, after cargo test
    { id: 'conformance-self-test', name: 'conformance runner self-test (published Ed25519 and JCS vectors)', cmd: ['node', 'tests/protocol/runner/run.mjs', '--self-test'], sandbox: true },
    { id: 'conformance', name: 'conformance fixtures and requirement indexes (reference runner)', cmd: ['node', 'tests/protocol/runner/run.mjs'], sandbox: true },
    { id: 'fake-codex-self-test', name: 'fake Codex app-server self-test (stdio and loopback WebSocket)', cmd: ['node', 'tests/fakes/codex-app-server/self-test.mjs'], sandbox: true },
    { id: 'compiled-tests', name: 'security suite threat map vs compiled tests', cmd: ['node', 'tests/security/check-compiled-tests.mjs'], offline: true, sandbox: true },
    { id: 'compiled-tests-self-test', name: 'security suite threat map check self-test', cmd: ['node', 'tests/security/check-compiled-tests.mjs', '--self-test'], offline: true, sandbox: true },
    // boundary-lint.yml job `boundary-lint`, the rg checks (verbatim in .github/local-ci.sh)
    { id: 'boundary-check-3', name: 'check 3: no provider SDK imports or deps in the code tree', bash: 'boundary-check-3', needs: ['rg'] },
    { id: 'boundary-check-8', name: 'check 8: no separately administered server for local use', bash: 'boundary-check-8', needs: ['rg'] },
    { id: 'boundary-checks-1-2', name: 'checks 1-2 and spec neutral vocabulary over spec/ and core/', bash: 'boundary-checks-1-2', needs: ['rg'] },
    { id: 'boundary-check-11', name: 'check 11: no Beacon in product paths', bash: 'boundary-check-11', needs: ['rg'] },
    // this runner's own coverage, plan, policy and planted rg checks
    { id: 'local-ci-self-test', name: 'local-ci self-test (coverage, plan, commands, policy, planted rg checks)', cmd: ['node', 'scripts/local-ci.mjs', '--self-test'], needs: ['rg'] },
  ],
  // keystore-optin.yml (#52, F3): each test writes and deletes an entry of OAC's own under the
  // service io.github.rossgraeber.oac.test, never the device key's entry, never a harness's.
  keystore: [
    toolchain,
    {
      id: 'keystore-os-store',
      name: 'real credential store round trip (Credential Manager / Keychain)',
      cmd: ['cargo', 'test', '-p', 'oac-cli', '--locked', 'real_credential_store_round_trip', '--', '--ignored', '--nocapture'],
      env: { OAC_TEST_REAL_KEYRING: '1' },
      os: ['win32', 'darwin'],
      osReason: 'the Linux legs below cover this OS',
    },
    { id: 'keystore-build', name: 'build the test first, outside the session bus', cmd: ['cargo', 'test', '-p', 'oac-cli', '--locked', '--no-run'], os: ['linux'], osReason: 'Linux legs only' },
    {
      id: 'keystore-secret-service',
      name: 'real Secret Service round trip (throwaway gnome-keyring in a private session bus)',
      cmd: ['dbus-run-session', '--', 'sh', '-c', 'printf "throwaway" | gnome-keyring-daemon --unlock --components=secrets >/dev/null && cargo test -p oac-cli --locked real_credential_store_round_trip -- --ignored --nocapture'],
      env: { OAC_TEST_REAL_KEYRING: '1' },
      os: ['linux'],
      osReason: 'Linux legs only',
      needs: ['dbus-run-session', 'gnome-keyring-daemon'],
      install: 'sudo apt-get install -y gnome-keyring dbus-x11',
    },
    {
      id: 'keystore-headless',
      name: 'headless fallback to the encrypted file (no session bus)',
      cmd: ['env', '-u', 'DBUS_SESSION_BUS_ADDRESS', '-u', 'XDG_RUNTIME_DIR', 'cargo', 'test', '-p', 'oac-cli', '--locked', 'headless_linux_falls_back_to_the_encrypted_file', '--', '--ignored', '--nocapture'],
      env: { OAC_TEST_REAL_KEYRING: '1' },
      os: ['linux'],
      osReason: 'Linux legs only',
    },
  ],
  // scale-optin.yml (#328)
  scale: [
    toolchain,
    fetch,
    { id: 'scale-full', name: 'full-scale bound tests (release, ignored by default)', cmd: ['cargo', 'test', '-p', 'oac-core', '--release', '--locked', 'full_scale', '--', '--ignored'], offline: true },
  ],
  // security-mutation-optin.yml (#329)
  mutation: [
    toolchain,
    fetch,
    loopbackSelect,
    loopbackProbe,
    { id: 'mutation-check', name: 'mutation check (every planted core regression must be caught)', cmd: ['node', 'tests/security/mutation-check.mjs', '--work-dir', '<work>/security-mutation'], offline: true, sandbox: true },
  ],
  // g3-macos-hosted.yml (#219)
  'g3-macos': [{ id: 'g3-macos', name: 'G3 Zenoh peer Mac leg (6 scenarios x 3, extras); results in <work>/g3-out', bash: 'g3-macos', os: ['darwin'], osReason: 'macOS only: sw_vers, lo0, sysctl; no Mac here (lost coverage)', notRunElsewhere: true, needs: ['python3', 'openssl'] }],
};

// The repository paths each step depends on, by step id: a change under none of them cannot
// change the step's result. Not used to skip anything yet; issue #362 (change-scoped runs,
// with a `--full` flag) reads it. Entries are repo-relative: `dir/` is a directory prefix,
// a plain path is one file, `**` is any path. --self-test requires an entry for every step.
const RUST = ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', '.cargo/', 'core/', 'cli/', 'adapters/', 'transports/', 'tests/'];
const PRODUCT = ['adapters/', 'core/', 'cli/', 'transports/', 'spec/'];
export const PATHS = {
  toolchain: ['rust-toolchain.toml'],
  fmt: [...RUST, 'rustfmt.toml', '.rustfmt.toml'],
  'cargo-fetch': ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml'],
  build: RUST,
  clippy: [...RUST, 'clippy.toml', '.clippy.toml'],
  'loopback-select': ['scripts/loopback-only.sh'],
  'loopback-probe': ['scripts/loopback-only.sh'],
  'cargo-test': [...RUST, 'spec/', 'docs/planning/gates/fixtures/'],
  'crate-deps-self-test': ['scripts/check-crate-deps.mjs'],
  'crate-deps': [...RUST, 'scripts/check-crate-deps.mjs'],
  'crate-deps-mutation': [...RUST, 'scripts/check-crate-deps.mjs'],
  'crate-deps-adapters-alone': [...RUST, 'scripts/check-crate-deps.mjs'],
  'licenses-self-test': ['scripts/check-licenses.mjs', 'scripts/check-crate-deps.mjs'],
  'licenses-mutation': [...RUST, 'scripts/check-licenses.mjs', 'scripts/check-crate-deps.mjs'],
  licenses: [...RUST, 'scripts/', 'tools/', 'scripts/check-licenses.mjs'],
  'containment-self-test': ['scripts/check-containment.mjs'],
  containment: [...PRODUCT, 'tests/fakes/', 'tests/protocol/', 'Cargo.toml', 'Cargo.lock', 'scripts/check-containment.mjs'],
  'fixture-manifest-self-test': ['scripts/check-fixture-manifest.mjs'],
  'fixture-manifest': ['docs/planning/gates/', 'docs/planning/PINS.md', 'scripts/check-fixture-manifest.mjs'],
  'workflows-self-test': ['scripts/check-workflows.mjs'],
  workflows: ['.github/', 'scripts/check-workflows.mjs'],
  'herdr-containment-self-test': ['scripts/check-herdr-containment.mjs'],
  'herdr-containment': [...PRODUCT, 'tools/herdr/', 'tests/integration/', '.github/workflows/', '.gitmodules', '**/Cargo.toml', '**/Cargo.lock', '**/package.json', '**/package-lock.json', 'scripts/check-herdr-containment.mjs'],
  skills: ['.claude/skills/', 'CLAUDE.md', 'scripts/check-skills.mjs'],
  'agents-skills-sync-self-test': ['scripts/sync-agents-skills.mjs'],
  'agents-skills-sync': ['.claude/skills/', 'CLAUDE.md', '.agents/', 'AGENTS.md', 'scripts/sync-agents-skills.mjs'],
  'herdr-self-test': ['tools/herdr/', 'docs/planning/PINS.md'],
  'conformance-self-test': ['tests/protocol/runner/'],
  conformance: ['tests/protocol/', 'spec/'],
  'fake-codex-self-test': ['tests/fakes/codex-app-server/'],
  'compiled-tests': [...RUST, 'tests/security/'],
  'compiled-tests-self-test': [...RUST, 'tests/security/'],
  'boundary-check-3': ['**', '.github/local-ci.sh'],
  'boundary-check-8': ['**', '.github/local-ci.sh'],
  'boundary-checks-1-2': ['spec/', 'core/', '.github/local-ci.sh'],
  'boundary-check-11': [...PRODUCT, 'Cargo.toml', 'Cargo.lock', '.github/local-ci.sh'],
  'local-ci-self-test': ['scripts/', '.github/local-ci.sh', 'tools/herdr/test/selftest.mjs'],
  'keystore-os-store': ['cli/', 'core/', 'Cargo.toml', 'Cargo.lock'],
  'keystore-build': ['cli/', 'core/', 'Cargo.toml', 'Cargo.lock'],
  'keystore-secret-service': ['cli/', 'core/', 'Cargo.toml', 'Cargo.lock'],
  'keystore-headless': ['cli/', 'core/', 'Cargo.toml', 'Cargo.lock'],
  'scale-full': ['core/', 'Cargo.toml', 'Cargo.lock'],
  'mutation-check': ['core/', 'tests/security/', 'Cargo.toml', 'Cargo.lock'],
  'g3-macos': ['docs/planning/gates/fixtures/g3-zenoh-peer/', '.github/local-ci.sh'],
};
for (const steps of Object.values(TIERS)) for (const s of steps) s.paths = PATHS[s.id];

// Every step of every deleted workflow -> the step here that runs it. Setup-only steps
// (checkout, cache, artifact upload, ripgrep install) have no counterpart; they are named so
// the list is complete.
export const PORTED = [
  ['ci.yml test: Install the pinned toolchain from rust-toolchain.toml', 'default', 'toolchain'],
  ['ci.yml test: cargo fmt --check', 'default', 'fmt'],
  ['ci.yml test: cargo fetch', 'default', 'cargo-fetch'],
  ['ci.yml test: cargo build', 'default', 'build'],
  ['ci.yml test: cargo clippy -D warnings', 'default', 'clippy'],
  ['ci.yml test: Select the loopback-only sandbox', 'default', 'loopback-select'],
  ['ci.yml test: Loopback-only sandbox probe', 'default', 'loopback-probe'],
  ['ci.yml test: Unit, contract, security, fake-integration and conformance tiers (cargo test)', 'default', 'cargo-test'],
  ['ci.yml test: Security suite threat map vs compiled tests', 'default', 'compiled-tests'],
  ['ci.yml test: Security suite threat map check self-test', 'default', 'compiled-tests-self-test'],
  ['ci.yml test: Conformance runner self-test', 'default', 'conformance-self-test'],
  ['ci.yml test: Conformance fixtures and requirement indexes', 'default', 'conformance'],
  ['ci.yml test: Fake Codex app-server self-test', 'default', 'fake-codex-self-test'],
  ['ci.yml herdr-selftest: herdr driver self-test', 'default', 'herdr-self-test'],
  ['ci.yml crate-deps: Dependency direction self-test', 'default', 'crate-deps-self-test'],
  ['ci.yml crate-deps: Dependency direction', 'default', 'crate-deps'],
  ['ci.yml crate-deps: Dependency direction mutation test', 'default', 'crate-deps-mutation'],
  ['ci.yml crate-deps: Adapters build alone (PR #352)', 'default', 'crate-deps-adapters-alone'],
  ['ci.yml licenses: License check self-test', 'default', 'licenses-self-test'],
  ['ci.yml licenses: License check mutation test', 'default', 'licenses-mutation'],
  ['ci.yml licenses: License headers and dependency inventory', 'default', 'licenses'],
  ['boundary-lint.yml boundary-lint: Checks 9-10 self-test', 'default', 'herdr-containment-self-test'],
  ['boundary-lint.yml boundary-lint: Checks 9-10 herdr containment', 'default', 'herdr-containment'],
  ['boundary-lint.yml boundary-lint: Check 3', 'default', 'boundary-check-3'],
  ['boundary-lint.yml boundary-lint: Check 8', 'default', 'boundary-check-8'],
  ['boundary-lint.yml boundary-lint: Checks 1-2 and spec neutral vocabulary', 'default', 'boundary-checks-1-2'],
  ['boundary-lint.yml boundary-lint: Check 11', 'default', 'boundary-check-11'],
  ['boundary-lint.yml fixture-manifest: Fixture manifest self-test', 'default', 'fixture-manifest-self-test'],
  ['boundary-lint.yml fixture-manifest: Fixture manifest check', 'default', 'fixture-manifest'],
  ['boundary-lint.yml containment: Checks 12-13 self-test', 'default', 'containment-self-test'],
  ['boundary-lint.yml containment: Check 12 and check 13', 'default', 'containment'],
  ['boundary-lint.yml workflow-policy: Workflow policy self-test', 'default', 'workflows-self-test'],
  ['boundary-lint.yml workflow-policy: Workflow policy', 'default', 'workflows'],
  ['boundary-lint.yml agents-skills-sync: Codex skills copy self-test', 'default', 'agents-skills-sync-self-test'],
  ['boundary-lint.yml agents-skills-sync: Codex skills copy matches its source', 'default', 'agents-skills-sync'],
  ['boundary-lint.yml agents-skills-sync: Skill budgets', 'default', 'skills'],
  ['keystore-optin.yml (every job): Install the pinned toolchain from rust-toolchain.toml', 'keystore', 'toolchain'],
  ['keystore-optin.yml os-credential-store: Real credential store round trip', 'keystore', 'keystore-os-store'],
  ['keystore-optin.yml linux-secret-service: Build the test first, outside the session bus', 'keystore', 'keystore-build'],
  ['keystore-optin.yml linux-secret-service: Real Secret Service round trip', 'keystore', 'keystore-secret-service'],
  ['keystore-optin.yml linux-headless-fallback: Headless fallback to the encrypted file', 'keystore', 'keystore-headless'],
  ['scale-optin.yml full-scale: Install the pinned toolchain from rust-toolchain.toml', 'scale', 'toolchain'],
  ['scale-optin.yml full-scale: cargo fetch', 'scale', 'cargo-fetch'],
  ['scale-optin.yml full-scale: Full-scale bound tests', 'scale', 'scale-full'],
  ['security-mutation-optin.yml mutation-check: Install the pinned toolchain from rust-toolchain.toml', 'mutation', 'toolchain'],
  ['security-mutation-optin.yml mutation-check: cargo fetch', 'mutation', 'cargo-fetch'],
  ['security-mutation-optin.yml mutation-check: Select the loopback-only sandbox', 'mutation', 'loopback-select'],
  ['security-mutation-optin.yml mutation-check: Loopback-only sandbox probe', 'mutation', 'loopback-probe'],
  ['security-mutation-optin.yml mutation-check: Mutation check', 'mutation', 'mutation-check'],
  ['g3-macos-hosted.yml g3-macos: every step (record, venv, stage, certs, matrix, extras)', 'g3-macos', 'g3-macos'],
];
// Setup steps with no counterpart, by design.
export const NOT_PORTED = [
  'actions/checkout (the developer\'s own checkout; fixture-manifest needs full history, as fetch-depth: 0 gave)',
  'actions/cache restore/save (cargo\'s own target/ and registry cache)',
  'Install ripgrep (rg must be on PATH; NOT RUN otherwise)',
  'actions/upload-artifact in g3-macos-hosted.yml (results stay in the local work directory)',
];

// ---- environment ----------------------------------------------------------------------

function onPath(program) {
  const r = spawnSync(process.platform === 'win32' ? 'where' : 'sh', process.platform === 'win32' ? [program] : ['-c', `command -v ${program}`], { encoding: 'utf8' });
  return r.status === 0;
}

// Git Bash on Windows: never the WSL launcher (System32 or WindowsApps bash.exe).
function findBash() {
  if (process.platform !== 'win32') return 'bash';
  const candidates = [];
  if (process.env.OAC_BASH) candidates.push(process.env.OAC_BASH);
  const exec = spawnSync('git', ['--exec-path'], { encoding: 'utf8' });
  if (exec.status === 0) {
    const p = exec.stdout.trim();
    candidates.push(resolve(p, '..', '..', '..', 'bin', 'bash.exe'), resolve(p, '..', '..', '..', 'usr', 'bin', 'bash.exe'));
  }
  candidates.push('C:\\Program Files\\Git\\bin\\bash.exe');
  return candidates.find((c) => existsSync(c) && !/\\(?:System32|WindowsApps)\\/i.test(c)) ?? null;
}

const git = (args) => {
  const r = spawnSync('git', args, { cwd: repoRoot, encoding: 'utf8' });
  return r.status === 0 ? r.stdout.trim() : null;
};

// ---- the plan -------------------------------------------------------------------------

// The concrete action for each step on `platform`: { step, skip?, notRun?, argv?, env, bash? }.
export function plan(tier, { platform, quick, work, only = null }) {
  const steps = TIERS[tier];
  return steps
    .filter((s) => !only || only.includes(s.id))
    .map((s) => {
      if (s.os && !s.os.includes(platform)) return { step: s, [s.notRunElsewhere ? 'notRun' : 'skip']: s.osReason };
      const env = { ...(s.offline ? OFFLINE : {}), ...(s.env ?? {}), ...(quick && s.quickEnv ? s.quickEnv : {}) };
      if (s.check) return { step: s, check: s.check, env };
      if (s.bash) return { step: s, bash: s.bash, env: s.bash === 'g3-macos' ? { ...env, G3_WORK: work } : env };
      let argv = s.cmd.map((a) => a.replace('<work>', work));
      if (s.sandbox && platform === 'linux') argv = ['bash', 'scripts/loopback-only.sh', ...argv];
      return { step: s, argv, env };
    });
}

// ---- running --------------------------------------------------------------------------

function run(actions, ctx) {
  const results = [];
  let sandboxOk = null; // Linux: null until loopback-select ran
  for (const a of actions) {
    const t0 = Date.now();
    const done = (status, note = '') => {
      results.push({ id: a.step.id, name: a.step.name, status, note, ms: Date.now() - t0 });
      console.log(`\n=== local-ci: ${a.step.id}: ${status}${note ? ` (${note})` : ''}\n`);
    };
    console.log(`\n=== local-ci: ${a.step.id}: ${a.step.name}`);
    if (a.skip) { done('SKIP', a.skip); continue; }
    if (a.notRun) { done('NOT RUN', a.notRun); continue; }
    const missing = (a.step.needs ?? []).filter((p) => !onPath(p));
    if (missing.length) { done('NOT RUN', `missing on PATH: ${missing.join(', ')}${a.step.install ? `; install: ${a.step.install}` : ''}`); continue; }
    const env = { ...process.env, CARGO_INCREMENTAL: '0', ...a.env };
    if (a.check === 'toolchain') {
      const r = spawnSync('rustc', ['--version'], { cwd: repoRoot, encoding: 'utf8', env });
      const out = (r.stdout ?? '').trim();
      console.log(out || r.error?.message);
      done(r.status === 0 && out.startsWith(`rustc ${TOOLCHAIN} `) ? 'PASS' : 'FAIL', out || 'rustc not found');
      continue;
    }
    if (a.check === 'loopback-select') {
      const r = spawnSync(ctx.bash, ['scripts/loopback-only.sh', '--select'], { cwd: repoRoot, encoding: 'utf8', env });
      process.stdout.write(r.stdout ?? '');
      process.stderr.write(r.stderr ?? '');
      sandboxOk = r.status === 0 && /^LOOPBACK_ONLY=bash scripts\/loopback-only\.sh$/m.test(r.stdout ?? '');
      done(sandboxOk ? 'PASS' : 'FAIL', sandboxOk ? 'network namespace available' : 'cannot make a network namespace: the sandboxed steps fail');
      continue;
    }
    if (a.argv && a.argv[0] === 'bash' && a.argv[1] === 'scripts/loopback-only.sh' && ctx.platform === 'linux' && sandboxOk !== true) {
      done('FAIL', 'loopback-only sandbox unavailable (loopback-select failed or did not run)');
      continue;
    }
    let r;
    if (a.bash) {
      const body = readFileSync(join(repoRoot, BASH_BODIES), 'utf8').replace(/\r/g, '');
      r = spawnSync(ctx.bash, ['--noprofile', '--norc', '-eo', 'pipefail', '-s', '--', a.bash], { cwd: repoRoot, env, input: body, stdio: ['pipe', 'inherit', 'inherit'] });
    } else {
      const [prog, ...args] = a.argv;
      const program = prog === 'node' ? process.execPath : prog === 'bash' ? ctx.bash : prog;
      const nodeArgs = args.map((x) => (x === 'node' ? process.execPath : x));
      r = spawnSync(program, nodeArgs, { cwd: repoRoot, env, stdio: 'inherit' });
    }
    if (r.error) done('FAIL', r.error.message);
    else if (a.step.notRunElsewhere && r.status === 3) done('NOT RUN', 'the script reported it cannot run here');
    else done(r.status === 0 ? 'PASS' : 'FAIL', r.status === 0 ? '' : `exit ${r.status ?? r.signal}`);
  }
  return results;
}

const fmt = (ms) => (ms < 60000 ? `${(ms / 1000).toFixed(1)}s` : `${Math.floor(ms / 60000)}m${String(Math.round((ms % 60000) / 1000)).padStart(2, '0')}s`);

function summary(tier, results, ctx, totalMs) {
  const head = git(['rev-parse', 'HEAD']) ?? '(unknown)';
  const branch = git(['rev-parse', '--abbrev-ref', 'HEAD']) ?? '?';
  const dirty = (git(['status', '--porcelain', '--untracked-files=no']) ?? '').split('\n').filter(Boolean).length;
  const n = (s) => results.filter((r) => r.status === s).length;
  const verdict = n('FAIL') ? 'FAIL' : n('NOT RUN') ? 'INCOMPLETE' : 'PASS';
  const rustc = (spawnSync('rustc', ['--version'], { encoding: 'utf8' }).stdout ?? '').trim() || 'rustc not found';
  const overrides = Object.keys(process.env).filter((k) => /^CARGO_(?:HOME$|SOURCE|PATCH)/i.test(k)).sort();
  const lines = [
    `### local-ci: ${verdict} (tier ${tier}${ctx.quick ? ', --quick' : ''}${ctx.only ? ', PARTIAL --only' : ''})`,
    '',
    `- HEAD: \`${head}\` (${branch}); work tree ${dirty ? `DIRTY (${dirty} tracked file(s) changed): this is not a run of HEAD` : 'clean'}`,
    `- Platform: ${ctx.platform} ${process.arch}${ctx.wsl ? ' (WSL)' : ''}, OS ${osRelease()}; node ${process.version}; ${rustc}`,
    `- Loopback-only sandbox: ${ctx.platform === 'linux' ? 'used for the sandboxed steps' : 'not available on this OS (as on the old windows/macos CI legs)'}`,
    `- Cargo source overrides inherited from the environment (W6; the dependency checks trust it): ${overrides.length ? `**${overrides.join(', ')}** (review them)` : 'none'}`,
    `- ${n('PASS')} passed, ${n('FAIL')} failed, ${n('SKIP')} skipped, ${n('NOT RUN')} not run; ${fmt(totalMs)} in all`,
    '',
    '| Step | Result | Time |',
    '|---|---|---|',
    ...results.map((r) => `| \`${r.id}\` ${r.name} | ${r.status}${r.note ? `: ${r.note.replace(/\|/g, '/')}` : ''} | ${fmt(r.ms)} |`),
  ];
  return { text: lines.join('\n'), verdict };
}

// ---- self-test ------------------------------------------------------------------------

// D2/D3, moved here from scripts/check-workflows.mjs with the hosted default tier. The package
// names use a one-letter class so that check 3 does not match this file's own source.
const OPT_IN_SWITCH = /--(?:include-)?ignored\b|\bOAC_TEST_[A-Z0-9_]+|tools[\\/]+herdr/;
const HARNESS_INSTALL = /@anthropi[c]-ai\/claude-code|@open[a]i\/codex|\bclaude\.ai\/install|\b(?:npm|npx|pnpm|yarn|bun)\b[^\n]*\b(?:claude-code|codex)\b|\bbrew\s+install\b[^\n]*\b(?:codex|claude)\b/i;
const DRIVER_SELFTEST = 'node tools/herdr/run.mjs --self-test';
// W6 of scripts/check-workflows.mjs (PR #352): what can swap a crate's source behind the
// dependency checks. Refused in every step; the summary lists any such variable it inherits.
const CARGO_OVERRIDE = /(?:^|[^\w-])--config\b|\bCARGO_HOME\b|\bCARGO_(?:SOURCE|PATCH)\w*/i;

// Violations of the default-tier policy in `steps`.
export function policy(steps) {
  const v = [];
  let fetched = false;
  for (const s of steps) {
    const text = [...(s.cmd ?? []), s.bash ?? ''].join(' ');
    const envText = Object.entries({ ...(s.env ?? {}), ...(s.quickEnv ?? {}) }).map(([k, x]) => `${k}=${x}`).join(' ');
    const exempt = (s.cmd ?? []).join(' ') === DRIVER_SELFTEST && !s.env && Object.keys(s.quickEnv ?? {}).every((k) => k === 'OAC_HERDR_SELFTEST_UNIT_ONLY');
    if (!exempt && (OPT_IN_SWITCH.test(text) || OPT_IN_SWITCH.test(envText))) v.push(`${s.id}: opt-in switch in the default tier (D2)`);
    if (HARNESS_INSTALL.test(text)) v.push(`${s.id}: harness CLI install in the default tier (D3)`);
    // check-workflows.mjs W6's policy, for the command lines the dependency checks now trust.
    if (CARGO_OVERRIDE.test(`${text} ${envText}`)) v.push(`${s.id}: cargo source override (--config, CARGO_HOME, CARGO_SOURCE_*, CARGO_PATCH*; W6, G-7 section 5)`);
    if (s.cmd?.[0] === 'cargo') {
      if (s.cmd[1] === 'fetch') fetched = true;
      else if (s.cmd[1] !== 'fmt' && !s.offline) v.push(`${s.id}: cargo step without CARGO_NET_OFFLINE`);
      if (s.cmd[1] !== 'fmt' && s.cmd[1] !== 'fetch' && !fetched) v.push(`${s.id}: cargo step before the fetch`);
    }
    // The loopback sandbox steps are Linux-only by nature (network namespaces); nothing else.
    if (s.os && !s.id.startsWith('loopback-')) v.push(`${s.id}: a default-tier step limited to some OSes`);
  }
  return v;
}

// The planted boundary-lint cases: [section, name, files, expected exit].
const P = (...parts) => parts.join(''); // keeps planted words out of this file's own text
const BOUNDARY_CASES = [
  ['boundary-check-3', 'clean tree', { 'a.mjs': 'console.log(1);\n' }, 0],
  ['boundary-check-3', 'provider SDK import in a .mjs', { 'a.mjs': P('import ', 'open', 'ai from "x";\n') }, 1],
  ['boundary-check-3', 'provider SDK dependency in Cargo.toml', { 'Cargo.toml': P('[dependencies]\n', 'anthro', 'pic = "1"\n') }, 1],
  ['boundary-check-3', 'a file type outside its globs is not scanned', { 'notes.md': P('open', 'ai\n') }, 0],
  // As written, the later `--glob '*.json'` etc. override the earlier `!docs/**` and `!target`
  // (in ripgrep the last matching glob wins), so a docs/ file of a scanned type IS scanned.
  // Kept verbatim; this case pins the behaviour so a change to it is a visible decision.
  ['boundary-check-3', 'docs/*.json is scanned (the !docs/** glob is overridden)', { 'docs/a.json': P('{"', 'open', 'ai": 1}\n') }, 1],
  ['boundary-check-8', 'clean tree', { 'a.txt': 'loopback peer\n' }, 0],
  ['boundary-check-8', 'a router daemon named in a script', { 'run.sh': P('zenoh', 'd --config x\n') }, 1],
  ['boundary-check-8', 'a compose file named', { 'x.yml': P('docker', '-compose up\n') }, 1],
  ['boundary-check-8', 'docs/ is exempt', { 'docs/x.md': P('kuber', 'netes\n') }, 0],
  ['boundary-checks-1-2', 'clean spec and core', { 'spec/session-channels.md': 'Sessions exchange envelopes.\n', 'core/src/lib.rs': '// core\n' }, 0],
  ['boundary-checks-1-2', 'spec/ missing', { 'core/src/lib.rs': '// core\n' }, 1],
  ['boundary-checks-1-2', 'check 1: a transport name in core/', { 'spec/session-channels.md': 'ok\n', 'core/src/lib.rs': P('// ', 'zen', 'oh key\n') }, 1],
  ['boundary-checks-1-2', 'check 2: a provider method in spec/', { 'spec/session-channels.md': P('call ', 'turn/', 'start\n') }, 1],
  ['boundary-checks-1-2', 'neutral vocabulary: a provider name in spec/', { 'spec/session-channels.md': P('the ', 'cod', 'ex harness\n') }, 1],
  ['boundary-checks-1-2', 'the binding document is exempt from check 2', { 'spec/session-channels.md': 'ok\n', 'spec/bindings/mcp.md': P('maps to ', 'tools/', 'call\n') }, 0],
  ['boundary-check-11', 'no product paths: PENDING exits 0', { 'README.md': '# x\n' }, 0],
  ['boundary-check-11', 'clean product path', { 'core/src/lib.rs': '// core\n' }, 0],
  ['boundary-check-11', 'Beacon in a product file', { 'core/src/lib.rs': P('// uses ', 'bea', 'con\n') }, 1],
  ['boundary-check-11', 'Beacon in a product path name', { [P('core/src/', 'bea', 'con_store.rs')]: '// x\n' }, 1],
];

function selfTest() {
  let failed = 0;
  const check = (name, ok, detail = '') => {
    console.log(`${ok ? 'ok  ' : 'FAIL'} ${name}${ok || !detail ? '' : ` -> ${detail}`}`);
    if (!ok) failed++;
  };

  // 1. coverage
  for (const [what, tier, id] of PORTED) check(`coverage: ${what} -> ${tier}/${id}`, TIERS[tier]?.some((s) => s.id === id));
  const ported = new Set(PORTED.map(([, t, id]) => `${t}/${id}`));
  for (const [tier, steps] of Object.entries(TIERS)) {
    const ids = steps.map((s) => s.id);
    check(`coverage: ${tier} step ids are unique`, new Set(ids).size === ids.length, ids.join(','));
    for (const s of steps) if (s.id !== 'local-ci-self-test') check(`coverage: ${tier}/${s.id} traces to a deleted workflow step`, ported.has(`${tier}/${s.id}`));
    for (const s of steps) check(`paths: ${tier}/${s.id} names the paths it depends on (#362)`, Array.isArray(s.paths) && s.paths.length > 0 && s.paths.every((p) => typeof p === 'string' && p && !p.startsWith('/')));
  }
  const allIds = new Set(Object.values(TIERS).flat().map((s) => s.id));
  for (const id of Object.keys(PATHS)) check(`paths: entry ${id} belongs to a step`, allIds.has(id));

  // 2. plan, against a recording stub
  for (const platform of ['linux', 'win32', 'darwin']) {
    const full = plan('default', { platform, quick: false, work: '/w' });
    const quick = plan('default', { platform, quick: true, work: '/w' });
    check(`plan ${platform}: every default step is planned`, full.length === TIERS.default.length);
    const skipped = full.filter((a) => a.skip || a.notRun).map((a) => a.step.id);
    const want = platform === 'linux' ? [] : ['loopback-select', 'loopback-probe'];
    check(`plan ${platform}: only ${want.join(', ') || 'nothing'} skipped`, JSON.stringify(skipped) === JSON.stringify(want), skipped.join(','));
    for (const a of full.filter((x) => x.argv)) {
      const wrapped = a.argv[0] === 'bash' && a.argv[1] === 'scripts/loopback-only.sh' && a.step.id !== 'loopback-probe';
      check(`plan ${platform}: ${a.step.id} ${a.step.sandbox && platform === 'linux' ? 'runs inside' : 'runs outside'} the loopback wrapper`, wrapped === Boolean(a.step.sandbox && platform === 'linux'));
    }
    const diff = full.filter((a, i) => JSON.stringify(a) !== JSON.stringify(quick[i])).map((a) => a.step.id);
    check(`plan ${platform}: --quick changes only the herdr self-test`, JSON.stringify(diff) === '["herdr-self-test"]', diff.join(','));
    const hq = quick.find((a) => a.step.id === 'herdr-self-test');
    const hf = full.find((a) => a.step.id === 'herdr-self-test');
    check(`plan ${platform}: --quick sets OAC_HERDR_SELFTEST_UNIT_ONLY=1, the full run does not`, hq.env.OAC_HERDR_SELFTEST_UNIT_ONLY === '1' && !('OAC_HERDR_SELFTEST_UNIT_ONLY' in hf.env));
    // A stub run: every planned step reaches the runner, in order.
    const seen = [];
    for (const a of full) if (!a.skip && !a.notRun) seen.push(a.step.id);
    const expected = TIERS.default.map((s) => s.id).filter((id) => !want.includes(id));
    check(`plan ${platform}: the stub runner sees every step, in order`, JSON.stringify(seen) === JSON.stringify(expected));
  }
  check('plan: g3-macos is NOT RUN off macOS and runs on macOS', plan('g3-macos', { platform: 'linux', work: '/w' })[0].notRun && plan('g3-macos', { platform: 'darwin', work: '/w' })[0].bash === 'g3-macos');
  check('plan: mutation runs in the loopback wrapper on Linux, with a work dir', plan('mutation', { platform: 'linux', work: '/w' }).find((a) => a.step.id === 'mutation-check').argv.join(' ') === 'bash scripts/loopback-only.sh node tests/security/mutation-check.mjs --work-dir /w/security-mutation');

  // 3. commands: every script exists, every flag is in it
  const bashBodies = readFileSync(join(repoRoot, BASH_BODIES), 'utf8');
  for (const [tier, steps] of Object.entries(TIERS)) {
    for (const s of steps) {
      if (s.bash) {
        check(`commands: ${tier}/${s.id}: section ${s.bash} exists in ${BASH_BODIES}`, new RegExp(`^${s.bash}\\)`, 'm').test(bashBodies));
        continue;
      }
      if (!s.cmd) continue;
      const script = s.cmd.find((a) => /\.(?:mjs|sh)$/.test(a));
      if (!script) continue;
      const path = join(repoRoot, script);
      const ok = existsSync(path);
      check(`commands: ${tier}/${s.id}: ${script} exists`, ok);
      if (!ok) continue;
      const src = readFileSync(path, 'utf8');
      for (const flag of s.cmd.filter((a) => a.startsWith('--') && a !== '--')) check(`commands: ${tier}/${s.id}: ${script} handles ${flag}`, src.includes(`'${flag}'`) || src.includes(`"${flag}"`) || src.includes(`${flag})`) || src.includes(`= "${flag}"`));
    }
  }
  check('commands: the herdr self-test honours OAC_HERDR_SELFTEST_UNIT_ONLY', readFileSync(join(repoRoot, 'tools/herdr/test/selftest.mjs'), 'utf8').includes("process.env.OAC_HERDR_SELFTEST_UNIT_ONLY === '1'"));

  // 4. policy, real and planted
  check('policy: the default tier is clean', policy(TIERS.default).length === 0, policy(TIERS.default).join('; '));
  const plant = (step) => policy([...TIERS.default, step]).length === 1;
  check('policy: an ignored test run is caught', plant({ id: 'x', cmd: ['cargo', 'test', '--', '--ignored'], offline: true }));
  check('policy: --include-ignored is caught', plant({ id: 'x', cmd: ['cargo', 'test', '--', '--include-ignored'], offline: true }));
  check('policy: an OAC_TEST_* opt-in is caught', plant({ id: 'x', cmd: ['cargo', 'test', '-p', 'oac-cli'], offline: true, env: { OAC_TEST_REAL_KEYRING: '1' } }));
  check('policy: a herdr scenario run is caught', plant({ id: 'x', cmd: ['node', 'tools/herdr/run.mjs', '--scenario', 'smoke'] }));
  check('policy: the herdr self-test with an extra argument is caught', plant({ id: 'x', cmd: ['node', 'tools/herdr/run.mjs', '--self-test', '--scenario', 'smoke'] }));
  check('policy: the herdr self-test with a step env is caught', plant({ id: 'x', cmd: ['node', 'tools/herdr/run.mjs', '--self-test'], env: { NODE_OPTIONS: '--require ./x.js' } }));
  check('policy: another driver entry point is caught', plant({ id: 'x', cmd: ['node', 'tools/herdr/ci.mjs', 'run'] }));
  check('policy: a harness CLI install is caught', plant({ id: 'x', cmd: ['npm', 'install', '-g', P('@open', 'ai/codex')] }));
  check('policy: a cargo step without CARGO_NET_OFFLINE is caught', plant({ id: 'x', cmd: ['cargo', 'build'] }));
  check('policy: a default step limited to one OS is caught', plant({ id: 'x', cmd: ['node', 'x.mjs'], os: ['linux'] }));
  check('policy: a cargo --config source replacement is caught (W6)', plant({ id: 'x', cmd: ['cargo', '--config', 'source.crates-io.replace-with="v"', 'build'], offline: true }));
  check('policy: CARGO_HOME in a step env is caught (W6)', plant({ id: 'x', cmd: ['node', 'x.mjs'], env: { CARGO_HOME: '/tmp/h' } }));
  check('policy: CARGO_PATCH* in a step env is caught (W6)', plant({ id: 'x', cmd: ['node', 'x.mjs'], env: { cargo_patch_crates_io_tokio_path: 'x' } }));
  check('policy: --configure is not a cargo --config (control)', policy([...TIERS.default, { id: 'x', cmd: ['node', 'x.mjs', '--configure-only'] }]).length === 0);
  check('policy: it is held to the default tier only (the keystore tier, an opt-in, would fail it)', policy(TIERS.keystore).some((x) => x.includes('D2')));

  // 5. planted boundary-lint checks in throwaway git trees
  const bash = findBash();
  check('boundary: bash found (Git Bash on Windows)', Boolean(bash));
  check('boundary: rg on PATH', onPath('rg'));
  if (bash && onPath('rg')) {
    const body = bashBodies.replace(/\r/g, '');
    for (const [section, name, files, want] of BOUNDARY_CASES) {
      const dir = mkdtempSync(join(tmpdir(), 'oac-local-ci-'));
      try {
        for (const [rel, text] of Object.entries(files)) {
          mkdirSync(dirname(join(dir, rel)), { recursive: true });
          writeFileSync(join(dir, rel), text);
        }
        spawnSync('git', ['init', '-q'], { cwd: dir });
        spawnSync('git', ['add', '-A'], { cwd: dir });
        const r = spawnSync(bash, ['--noprofile', '--norc', '-eo', 'pipefail', '-s', '--', section], { cwd: dir, input: body, encoding: 'utf8' });
        check(`boundary: ${section}: ${name} exits ${want}`, r.status === want, `exit ${r.status}: ${(r.stdout + r.stderr).slice(-300)}`);
      } finally {
        rmSync(dir, { recursive: true, force: true });
      }
    }
  }

  console.log(`\nlocal-ci self-test: ${failed ? `${failed} FAILED` : 'all passed'}`);
  return failed ? 1 : 0;
}

// ---- main -----------------------------------------------------------------------------

function usage(msg) {
  if (msg) console.error(`local-ci: ${msg}`);
  console.error('usage: node scripts/local-ci.mjs [--quick] [--tier default|keystore|scale|mutation|g3-macos] [--only id,...] [--list] | --self-test');
  process.exit(2);
}

function main(argv) {
  let tier = 'default';
  let quick = false;
  let list = false;
  let only = null;
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--self-test') {
      if (argv.length !== 1) usage('--self-test takes no other option');
      return selfTest();
    } else if (a === '--quick') quick = true;
    else if (a === '--list') list = true;
    else if (a === '--tier') {
      tier = argv[++i];
      if (!TIERS[tier]) usage(`unknown tier ${JSON.stringify(tier)}`);
    } else if (a === '--only') {
      only = (argv[++i] ?? '').split(',').filter(Boolean);
      if (!only.length) usage('--only needs step ids');
    } else usage(`unknown option ${JSON.stringify(a)}`);
  }
  if (only) for (const id of only) if (!TIERS[tier].some((s) => s.id === id)) usage(`no step ${id} in tier ${tier}`);
  const platform = process.platform;
  if (list) {
    for (const s of TIERS[tier]) console.log(`${s.id.padEnd(30)} ${s.name}${s.os ? ` [${s.os.join(', ')} only]` : ''}`);
    return 0;
  }
  const bash = findBash();
  if (!bash) usage('Git Bash not found (set OAC_BASH to its bash.exe)');
  const work = join(repoRoot, 'target', 'local-ci');
  mkdirSync(work, { recursive: true });
  let wsl = false;
  try {
    wsl = platform === 'linux' && /microsoft/i.test(readFileSync('/proc/version', 'utf8'));
  } catch {}
  const ctx = { platform, quick, only, bash, wsl };
  const t0 = Date.now();
  const results = run(plan(tier, { platform, quick, work, only }), ctx);
  const { text, verdict } = summary(tier, results, ctx, Date.now() - t0);
  writeFileSync(join(work, `summary-${tier}.md`), `${text}\n`);
  console.log(`\n${text}\n\n(also written to target/local-ci/summary-${tier}.md)`);
  return verdict === 'PASS' ? 0 : verdict === 'FAIL' ? 1 : 3;
}

if (process.argv[1] && resolve(process.argv[1]) === scriptPath) process.exit(main(process.argv.slice(2)));
