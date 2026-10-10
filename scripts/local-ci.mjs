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
//        lan         tests that listen beyond loopback on purpose (OAC_TEST_LAN=1); not in
//                    any deleted workflow: they ran in the default tier until the default
//                    became loopback-only (Windows firewall prompts; refs #7)
//   node scripts/local-ci.mjs --list [--tier <name>]   # print the steps, run nothing
//   node scripts/local-ci.mjs --only <id>[,<id>...]    # run only these steps (the summary
//                                                      # says PARTIAL; not for a PR)
//   node scripts/local-ci.mjs --self-test      # prove every step still runs (see below)
//   --allow-cargo-config   do not fail on cargo config files outside the repository (they are
//                          still listed in the summary)
//   --allow-env            do not fail on inherited variables that change what is built or
//                          run (still listed); inherited OAC_HERDR_SELFTEST_* / OAC_TEST_*
//                          always fail
//
// Cargo configuration outside the repository (PR #365 review B2, re-review): cargo also reads
// $CARGO_HOME/config(.toml) and .cargo/config(.toml) in every ancestor of the checkout. Any
// of them can swap a crate's source (`[source] replace-with`, `[patch]`, `paths`, in many
// TOML spellings: quoted keys, inline tables, dotted keys) behind check-crate-deps.mjs,
// --adapters-alone and the licence inventory (G-7 section 5, check-workflows.mjs W6). Hosted
// runners started clean; a developer machine does not. Step `cargo-config` fails on ANY such
// file, and on any repository-local `.cargo/config(.toml)` git does not track (root or member
// directory; an `[alias] clippy = "test --no-run"` there silently replaces a step, PR #365
// re-review B4), unless --allow-cargo-config; the keys it recognises are information only.
//
// Inherited environment (PR #365 re-reviews B3, B4): step `environment` fails on an inherited
// OAC_HERDR_SELFTEST_* or OAC_TEST_* (each would switch a step off or on unseen; --quick and
// the keystore tier set theirs for their own child only, and inherited ones are dropped from
// every child), and, unless --allow-env, on any CARGO_* outside an allowlist (CARGO_TERM_*,
// CARGO_HTTP_*, CARGO_NET_*, CARGO_LOG, CARGO_INCREMENTAL; every cargo config key has a
// CARGO_* form, CARGO_ALIAS_CLIPPY included), `CARGO`, NODE_OPTIONS, RUSTFLAGS, RUSTDOCFLAGS,
// RUSTC, RUSTDOC, RUSTC_WRAPPER, RUSTC_WORKSPACE_WRAPPER, RUSTC_BOOTSTRAP, and a
// RUSTUP_TOOLCHAIN other than rust-toolchain.toml's channel. The summary lists them, and
// CARGO_HOME. The summary's work-tree line counts untracked files too. A repository `.cargo`
// that is a symlink or junction is followed, as cargo follows it; a dangling one fails
// closed (PR #365 re-review B5).
//
// What remains local trust (accepted, not checked; PR #365 final re-review): the runner
// trusts PATH and rustup's toolchain resolution, and checks only `rustc --version`. So a
// PATH shim for cargo, rustc, node, bash, git or rg (a `cargo` shim could no-op clippy), a
// custom toolchain linked under rustup that reports 1.98.1, and a RUSTUP_HOME pointing at
// another rustup are not detected. Hosted runners started from a known image; a developer
// machine is trusted as it is.
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
//   - port contract (PR #365 review B1): scripts/local-ci.ported.json freezes, as data kept
//     apart from this file, every step of the deleted workflows (its old job and step, tier,
//     argv or bash section, env, sandbox flag, OS limit) with a row count and a sha256 of the
//     rows. The live plan must match it exactly, both ways: a dropped step, a weakened flag
//     (`--all-targets`, `-D warnings`, `--locked`), a lost CARGO_NET_OFFLINE or a lost
//     loopback wrapper fails, and so does an edit to the table that does not also update its
//     count and hash. Changing the contract is a visible edit to that file. Planted
//     mutations (the review's) must each be caught;
//   - plan: the plan is built for linux, win32 and darwin, full and --quick, and run against
//     a recording stub: every step is attempted, the sandboxed ones inside the loopback
//     wrapper on Linux, and --quick changes nothing but the herdr self-test's environment;
//   - commands: every script a step runs exists, and every flag it passes is in that script;
//   - policy (moved here from scripts/check-workflows.mjs D2 and D3 with the hosted default
//     tier): no default-tier step runs an ignored test, sets an OAC_TEST_* opt-in, reaches
//     the herdr driver beyond its exact offline self-test, or installs a harness CLI; every
//     cargo step after the fetch is offline; planted violations must be caught;
//   - boundary lint: each `rg` check of .github/local-ci.sh passes a clean throwaway git tree
//     and fails its planted violation;
//   - cargo config: a planted ancestor `.cargo/config.toml` with `[patch]` and a
//     $CARGO_HOME/config.toml with `replace-with` are found and flagged, a plain one is found
//     and not flagged.
//
// Node built-ins only. Exit codes: 0 = every step passed (SKIPs included); 1 = a step failed
// (or the self-test did); 2 = usage or environment error; 3 = no failure, but a step was
// NOT RUN (a missing prerequisite, or a tier this OS cannot run).

import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, lstatSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, symlinkSync, writeFileSync } from 'node:fs';
import { homedir, tmpdir, release as osRelease } from 'node:os';
import { dirname, join, parse as parsePath, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptPath = fileURLToPath(import.meta.url);
const repoRoot = resolve(dirname(scriptPath), '..');
const BASH_BODIES = '.github/local-ci.sh';
const PORTED_FILE = 'scripts/local-ci.ported.json';
// Steps of this runner's own, with no counterpart in a deleted workflow.
const LOCAL_ONLY = new Set(['local-ci-self-test', 'cargo-config', 'environment', 'test-listeners-self-test', 'test-listeners']);
// Tiers of this runner's own, with no counterpart in a deleted workflow: none of their steps
// has a row in the frozen port contract, and no ported row may name one (--self-test).
export const LOCAL_TIERS = new Set(['lan']);
const GUARDS = new Set(['cargo-config', 'environment']);
const TOOLCHAIN = '1.98.1'; // rust-toolchain.toml; docs/planning/PINS.md

// ---- steps ----------------------------------------------------------------------------
//
// A step: { id, name, cmd: [program, ...args] | bash: <section of .github/local-ci.sh> |
// check: <built-in check name>, env, sandbox (wrap in loopback-only.sh on Linux), offline
// (CARGO_NET_OFFLINE=true), os (only on these platforms; elsewhere SKIP with osReason),
// needs (programs that must be on PATH, else NOT RUN), quickEnv (env added under --quick),
// paths (set from PATHS below: what the step depends on), winWorkDir (on Windows, pass
// `--work-dir <os.tmpdir()>/<winWorkDir>-<run id>`: a short path, under MAX_PATH, of this
// run's own; the run id is this process's pid, so two overlapping runs, from one checkout or
// two, never share it. It is removed before the step and after it) }.
// `node` means this Node; `bash` means Git Bash on Windows.

const OFFLINE = { CARGO_NET_OFFLINE: 'true' };
const LINUX_ONLY_SANDBOX = 'not Linux: no network namespace; the rule rests on offline cargo and loopback-only fakes, as on the old windows/macos CI legs';

const toolchain = { id: 'toolchain', name: `rustc is the ${TOOLCHAIN} pin (rust-toolchain.toml)`, check: 'toolchain' };
const cargoConfig = { id: 'cargo-config', name: 'no cargo config file outside the repository or untracked in it (PR #365 B2, B4)', check: 'cargo-config' };
const environment = { id: 'environment', name: 'no inherited variable switches a step off or changes the build (PR #365 B3, B4)', check: 'environment' };
const fetch = { id: 'cargo-fetch', name: 'cargo fetch --locked (the one networked step)', cmd: ['cargo', 'fetch', '--locked'] };
const loopbackSelect = { id: 'loopback-select', name: 'loopback-only sandbox: select (mandatory on Linux)', check: 'loopback-select', os: ['linux'], osReason: LINUX_ONLY_SANDBOX };
const loopbackProbe = { id: 'loopback-probe', name: 'loopback-only sandbox: probe (loopback connects, nothing else is reachable)', cmd: ['bash', 'scripts/loopback-only.sh', '--probe'], os: ['linux'], osReason: LINUX_ONLY_SANDBOX };

export const TIERS = {
  default: [
    // ci.yml job `test`
    toolchain,
    cargoConfig,
    environment,
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
    // this runner's own (refs #7): nothing the default tier runs listens beyond loopback
    { id: 'test-listeners-self-test', name: 'loopback-only listeners self-test', cmd: ['node', 'scripts/check-test-listeners.mjs', '--self-test'] },
    { id: 'test-listeners', name: 'loopback-only listeners (opt-in LAN tests excepted)', cmd: ['node', 'scripts/check-test-listeners.mjs'] },
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
    { id: 'compiled-tests-self-test', name: 'security suite threat map check self-test', cmd: ['node', 'tests/security/check-compiled-tests.mjs', '--self-test'], offline: true, sandbox: true, winWorkDir: 'oac-cts' },
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
    cargoConfig,
    environment,
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
    cargoConfig,
    environment,
    fetch,
    { id: 'scale-full', name: 'full-scale bound tests (release, ignored by default)', cmd: ['cargo', 'test', '-p', 'oac-core', '--release', '--locked', 'full_scale', '--', '--ignored'], offline: true },
  ],
  // security-mutation-optin.yml (#329)
  mutation: [
    toolchain,
    cargoConfig,
    environment,
    fetch,
    loopbackSelect,
    loopbackProbe,
    { id: 'mutation-check', name: 'mutation check (every planted core regression must be caught)', cmd: ['node', 'tests/security/mutation-check.mjs', '--work-dir', '<work>/security-mutation'], offline: true, sandbox: true },
  ],
  // g3-macos-hosted.yml (#219)
  'g3-macos': [{ id: 'g3-macos', name: 'G3 Zenoh peer Mac leg (6 scenarios x 3, extras); results in <work>/g3-out', bash: 'g3-macos', os: ['darwin'], osReason: 'macOS only: sw_vers, lo0, sysctl; no Mac here (lost coverage)', notRunElsewhere: true, needs: ['python3', 'openssl'] }],
  // Tests that listen beyond loopback on purpose (scripts/check-test-listeners.mjs OPT_IN).
  // The default tier binds loopback only, so its runs raise no Windows firewall prompt; these
  // run here, with OAC_TEST_LAN=1 set for this child only. Not sandboxed: they need a LAN
  // interface. In the default tier the same tests print SKIPPED and pass.
  lan: [
    toolchain,
    cargoConfig,
    environment,
    fetch,
    {
      id: 'lan-zenoh-local-mode',
      name: 'Zenoh local mode reaches nothing beyond loopback (a LAN probe listens and scouts; PR #364 finding 1)',
      cmd: ['cargo', 'test', '-p', 'oac-transport-zenoh', '--locked', '--test', 'peer_transport', 'local_mode_reaches_nothing_beyond_loopback', '--', '--exact', '--nocapture'],
      env: { OAC_TEST_LAN: '1' },
      offline: true,
    },
  ],
};

// The repository paths each step depends on, by step id: a change under none of them cannot
// change the step's result. Not used to skip anything yet; issue #362 (change-scoped runs,
// with a `--full` flag) reads it. Entries are repo-relative: `dir/` is a directory prefix,
// a plain path is one file, `**` is any path. --self-test requires an entry for every step.
const RUST = ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', '.cargo/', 'core/', 'cli/', 'adapters/', 'transports/', 'tests/'];
const PRODUCT = ['adapters/', 'core/', 'cli/', 'transports/', 'spec/'];
export const PATHS = {
  toolchain: ['rust-toolchain.toml'],
  'cargo-config': ['**'], // and files outside the repository: never skip it
  environment: ['**'], // the caller's environment: never skip it
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
  'test-listeners-self-test': ['scripts/check-test-listeners.mjs'],
  'test-listeners': ['core/', 'cli/', 'adapters/', 'transports/', 'tests/', 'tools/', 'scripts/'],
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
  'lan-zenoh-local-mode': ['transports/zenoh/', 'core/', 'Cargo.toml', 'Cargo.lock'],
};
for (const steps of Object.values(TIERS)) for (const s of steps) s.paths = PATHS[s.id];

// ---- the port contract (PR #365 review B1) -------------------------------------------
//
// Every step of every deleted workflow is a row of scripts/local-ci.ported.json, kept apart
// from this file so one edit here cannot drop a step and its record together. A step's
// contract is everything that decides what it runs and where.
export function contractOf(step) {
  return {
    cmd: step.cmd ?? null,
    bash: step.bash ?? null,
    check: step.check ?? null,
    env: { ...(step.offline ? OFFLINE : {}), ...(step.env ?? {}) },
    quickEnv: step.quickEnv ?? {},
    sandbox: Boolean(step.sandbox),
    os: step.os ?? null,
    winWorkDir: step.winWorkDir ?? null,
  };
}
const canon = (v) => JSON.stringify(v, (_k, x) => (x && typeof x === 'object' && !Array.isArray(x) ? Object.fromEntries(Object.entries(x).sort(([a], [b]) => (a < b ? -1 : 1))) : x));
export const rowsHash = (rows) => createHash('sha256').update(JSON.stringify(rows), 'utf8').digest('hex');
export function loadPorted(text = readFileSync(join(repoRoot, PORTED_FILE), 'utf8')) {
  return JSON.parse(text);
}
// Violations of the frozen contract by `tiers`, both ways, plus the table's own count and hash.
export function portViolations(tiers, ported, localTiers = LOCAL_TIERS) {
  const v = [];
  const rows = Array.isArray(ported?.rows) ? ported.rows : [];
  if (ported?.count !== rows.length) v.push(`${PORTED_FILE}: count ${ported?.count} but ${rows.length} rows`);
  if (ported?.sha256 !== rowsHash(rows)) v.push(`${PORTED_FILE}: sha256 does not match its rows (edit rows, count and sha256 together)`);
  const seen = new Set();
  for (const r of rows) {
    const key = `${r.tier}/${r.id}`;
    if (seen.has(key)) v.push(`${key}: two rows`);
    if (localTiers.has(r.tier)) v.push(`${key}: a row for a tier of this runner's own (LOCAL_TIERS), which no deleted workflow had`);
    seen.add(key);
    const step = tiers[r.tier]?.find((s) => s.id === r.id);
    if (!step) {
      v.push(`${key} (${r.from}): no such step: a ported step was dropped`);
      continue;
    }
    const want = { cmd: r.cmd, bash: r.bash, check: r.check, env: r.env, quickEnv: r.quickEnv, sandbox: r.sandbox, os: r.os, winWorkDir: r.winWorkDir };
    const got = contractOf(step);
    for (const k of Object.keys(want)) if (canon(want[k]) !== canon(got[k])) v.push(`${key} (${r.from}): ${k} is ${canon(got[k])}, the contract says ${canon(want[k])}`);
  }
  for (const [tier, steps] of Object.entries(tiers)) {
    if (localTiers.has(tier)) continue;
    for (const s of steps) if (!LOCAL_ONLY.has(s.id) && !seen.has(`${tier}/${s.id}`)) v.push(`${tier}/${s.id}: a step with no row in ${PORTED_FILE}`);
  }
  // A tier of this runner's own is never the default (a ported tier is caught by its rows).
  if (localTiers.has('default')) v.push('default: LOCAL_TIERS names the default tier');
  return v;
}

// The PR #365 review's mutations, each of which the contract must catch.
export const PORT_MUTATIONS = [
  ['remove crate-deps-adapters-alone (its row stays in the frozen file)', (t) => { t.default = t.default.filter((s) => s.id !== 'crate-deps-adapters-alone'); }],
  ['drop --all-targets from cargo build', (t) => { const s = t.default.find((x) => x.id === 'build'); s.cmd = s.cmd.filter((a) => a !== '--all-targets'); }],
  ['drop -D warnings from clippy', (t) => { const s = t.default.find((x) => x.id === 'clippy'); s.cmd = s.cmd.slice(0, s.cmd.indexOf('--')); }],
  ['drop CARGO_NET_OFFLINE from --adapters-alone', (t) => { t.default.find((x) => x.id === 'crate-deps-adapters-alone').offline = false; }],
  ['drop the loopback wrapper from cargo test', (t) => { t.default.find((x) => x.id === 'cargo-test').sandbox = false; }],
  ['drop --locked from cargo test', (t) => { const s = t.default.find((x) => x.id === 'cargo-test'); s.cmd = s.cmd.filter((a) => a !== '--locked'); }],
  ['limit a default step to one OS', (t) => { t.default.find((x) => x.id === 'licenses').os = ['linux']; }],
  ['drop the herdr quick switch (so --quick no longer skips only the lifecycle half)', (t) => { delete t.default.find((x) => x.id === 'herdr-self-test').quickEnv; }],
  ['add an unrecorded step', (t) => { t.default.push({ id: 'extra', cmd: ['node', 'x.mjs'] }); }],
];
// Edits to the frozen file that must fail too.
export const TABLE_MUTATIONS = [
  ['remove a row, keep count and hash', (p) => { p.rows = p.rows.filter((r) => r.id !== 'crate-deps-adapters-alone'); }],
  ['remove a row and fix the count, keep the hash', (p) => { p.rows = p.rows.filter((r) => r.id !== 'crate-deps-adapters-alone'); p.count = p.rows.length; }],
  ['weaken a row, keep the hash', (p) => { p.rows.find((r) => r.id === 'clippy').cmd = ['cargo', 'clippy']; }],
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

// Cargo config files outside the repository that cargo reads for a build in `root`
// (https://doc.rust-lang.org/cargo/reference/config.html, "Hierarchical structure"):
// `.cargo/config` and `.cargo/config.toml` in every ancestor of `root`, and `config` /
// `config.toml` in $CARGO_HOME (default ~/.cargo). The repository's own .cargo/ is left to
// check-crate-deps.mjs rule 7. Each file found: { path, flagged: [what it holds] }, where
// flagged names the keys that can swap a crate's source: a `[source...]` or `[patch...]`
// table (or dotted `source.` / `patch.` keys), `replace-with`, or `paths`.
const SOURCE_KEYS = [
  [/^\s*\[\s*source\b/, '[source]'],
  [/^\s*\[\s*patch\b/, '[patch]'],
  [/^\s*source\s*\./, 'source.*'],
  [/^\s*patch\s*\./, 'patch.*'],
  [/\breplace-with\s*=/, 'replace-with'],
  [/^\s*paths\s*=/, 'paths'],
  [/^\s*\[\s*alias\b/, '[alias]'],
  [/^\s*alias\s*\./, 'alias.*'],
];
// { path, flagged } for one config file, or null when it is not a readable regular file.
function describeConfig(path) {
  let text;
  try {
    if (!statSync(path).isFile()) return null;
    text = readFileSync(path, 'utf8');
  } catch {
    return null;
  }
  // Information only (PR #365 re-review): TOML has too many spellings of a key for a line
  // match to be a gate. Quotes are dropped and inline tables split before matching.
  const lines = text.split(/\r?\n/).flatMap((l) => l.replace(/#.*$/, '').replace(/["']/g, '').split(/[{,]/));
  const flagged = SOURCE_KEYS.filter(([re]) => lines.some((l) => re.test(l) || re.test(`[${l.replace(/^\s*(\w[\w.-]*)\s*=.*$/, '$1')}`))).map(([, what]) => what);
  return { path, flagged };
}
// Repository-local `.cargo/config(.toml)` files git does not track (PR #365 re-review B4), at
// the root or in any member directory: cargo reads one when it runs there, and an untracked
// file is no reviewer-visible diff. (A tracked one is a diff, and check-crate-deps.mjs rule 7
// reads it.) target/, .git/ and node_modules/ are not searched.
export function repoLocalUntrackedConfigs(root = repoRoot) {
  const found = [];
  const walk = (dir, depth) => {
    let entries;
    try {
      entries = readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const e of entries) {
      if (['target', '.git', 'node_modules'].includes(e.name)) continue;
      const sub = join(dir, e.name);
      if (e.name === '.cargo') {
        // PR #365 re-review B5: a `.cargo` that is a symbolic link or a junction (a Dirent
        // reports isSymbolicLink() for both) is followed, as cargo follows it. One that is
        // dangling or cannot be resolved fails closed.
        let isDir = e.isDirectory();
        if (e.isSymbolicLink()) {
          try {
            isDir = statSync(sub).isDirectory();
          } catch {
            broken.push(sub);
            continue;
          }
        }
        if (!isDir) continue;
        for (const f of ['config', 'config.toml']) {
          const p = join(sub, f);
          let linkOrFile = false;
          try {
            lstatSync(p);
            linkOrFile = true;
          } catch {}
          if (!linkOrFile) continue;
          try {
            statSync(p);
            found.push(p);
          } catch {
            broken.push(p); // a dangling config link fails closed too
          }
        }
      } else if (e.isDirectory() && depth < 12) walk(sub, depth + 1); // other links are not entered
    }
  };
  const broken = [];
  walk(root, 0);
  const brokenEntries = broken.map((p) => ({ path: p, flagged: ['link that cannot be resolved (fails closed)'], local: true }));
  if (!found.length) return brokenEntries;
  const rel = found.map((p) => p.slice(resolve(root).length + 1).split('\\').join('/'));
  const tracked = new Set((spawnSync('git', ['ls-files', '-z', '--', ...rel], { cwd: root, encoding: 'utf8' }).stdout ?? '').split('\0').filter(Boolean));
  return [...found.filter((_p, i) => !tracked.has(rel[i])).map((p) => ({ ...(describeConfig(p) ?? { path: p, flagged: [] }), local: true })), ...brokenEntries];
}
export function cargoConfigFiles({ root = repoRoot, cargoHome = process.env.CARGO_HOME || join(homedir(), '.cargo') } = {}) {
  const candidates = [];
  let dir = dirname(resolve(root));
  const top = parsePath(dir).root;
  for (;;) {
    candidates.push(join(dir, '.cargo', 'config'), join(dir, '.cargo', 'config.toml'));
    if (dir === top) break;
    dir = dirname(dir);
  }
  candidates.push(join(cargoHome, 'config'), join(cargoHome, 'config.toml'));
  return [...new Set(candidates)].map(describeConfig).filter(Boolean);
}

const allCargoConfigs = () => [...cargoConfigFiles(), ...repoLocalUntrackedConfigs()];

// The gate (PR #365 re-reviews B2, B4): any cargo config file outside the repository, or
// untracked inside it, fails unless --allow-cargo-config. What a file holds is reported,
// never trusted to decide.
export function cargoConfigVerdict(files, allow) {
  if (!files.length) return { status: 'PASS', note: 'no cargo config outside the repository or untracked in it' };
  const list = files.map((f) => `${f.path}${f.local ? ' (untracked, in the repository)' : ''}${f.flagged.length ? ` (holds ${f.flagged.join(', ')})` : ''}`).join('; ');
  if (allow) return { status: 'PASS', note: `--allow-cargo-config: ${list}` };
  return { status: 'FAIL', note: `${list}: cargo reads ${files.length === 1 ? 'it' : 'them'} and any can swap a crate's source or alias a step's command (clippy -> test) behind the checks; move ${files.length === 1 ? 'it' : 'them'} aside or pass --allow-cargo-config` };
}

// Inherited environment (PR #365 re-reviews B3, B4). A step's own switches (the --quick env,
// the keystore tier's OAC_TEST_REAL_KEYRING) are set for that child only; inherited ones are
// refused and never passed on. Every config key of cargo has a CARGO_* environment form
// (CARGO_ALIAS_CLIPPY="test --no-run" turns `cargo clippy -- -D warnings` into a lint-free
// build), so CARGO_* is an allowlist, not a denylist: only CARGO_TERM_*, CARGO_HTTP_*,
// CARGO_NET_*, CARGO_LOG and CARGO_INCREMENTAL (which local-ci sets itself) pass, and
// CARGO_HOME is listed only (a relocated cargo home is common; step cargo-config reads it).
// Any other CARGO_*, `CARGO` itself, NODE_OPTIONS, the RUSTFLAGS family, RUSTC and its
// wrappers, RUSTC_BOOTSTRAP, and a RUSTUP_TOOLCHAIN other than rust-toolchain.toml's channel
// fail unless --allow-env.
const SWITCH_ENV = /^(?:OAC_HERDR_SELFTEST_|OAC_TEST_)/i;
const CARGO_ALLOWED = /^CARGO_(?:TERM_\w+|HTTP_\w+|NET_\w+|LOG|INCREMENTAL)$/i;
const BUILD_ENV = /^(?:NODE_OPTIONS|RUSTFLAGS|RUSTDOCFLAGS|RUSTC|RUSTC_WRAPPER|RUSTC_WORKSPACE_WRAPPER|RUSTC_BOOTSTRAP|RUSTDOC|CARGO)$/i;
const LISTED_ENV = /^CARGO_HOME$/i;
function toolchainChannel() {
  try {
    return /^\s*channel\s*=\s*["']([^"']+)["']/m.exec(readFileSync(join(repoRoot, 'rust-toolchain.toml'), 'utf8'))?.[1] ?? TOOLCHAIN;
  } catch {
    return TOOLCHAIN;
  }
}
export function envReport(env, channel = toolchainChannel()) {
  const keys = Object.keys(env).sort();
  const build = keys.filter((k) => {
    if (SWITCH_ENV.test(k) || LISTED_ENV.test(k)) return false;
    if (BUILD_ENV.test(k)) return true;
    if (/^CARGO_/i.test(k)) return !CARGO_ALLOWED.test(k);
    if (/^RUSTUP_TOOLCHAIN$/i.test(k)) return !(env[k] === channel || String(env[k]).startsWith(`${channel}-`));
    return false;
  });
  return { switches: keys.filter((k) => SWITCH_ENV.test(k)), build, listed: keys.filter((k) => LISTED_ENV.test(k)) };
}
export function envVerdict(env, allow, channel) {
  const r = envReport(env, channel);
  if (r.switches.length) return { status: 'FAIL', note: `inherited ${r.switches.join(', ')}: a switch only a step may set (--quick sets the herdr one for its child); unset ${r.switches.length === 1 ? 'it' : 'them'}` };
  if (r.build.length && !allow) return { status: 'FAIL', note: `inherited ${r.build.join(', ')} change${r.build.length === 1 ? 's' : ''} what is built or run; unset or pass --allow-env` };
  const parts = [...(r.build.length ? [`--allow-env: ${r.build.join(', ')}`] : []), ...(r.listed.length ? [`listed: ${r.listed.join(', ')}`] : [])];
  return { status: 'PASS', note: parts.join('; ') || 'nothing inherited that changes a step' };
}
// A child's environment: the caller's, minus every inherited switch, plus the step's own.
export function childEnv(base, stepEnv) {
  const env = Object.fromEntries(Object.entries(base).filter(([k]) => !SWITCH_ENV.test(k)));
  return { ...env, CARGO_INCREMENTAL: '0', ...stepEnv };
}

const git = (args) => {
  const r = spawnSync('git', args, { cwd: repoRoot, encoding: 'utf8' });
  return r.status === 0 ? r.stdout.trim() : null;
};

// ---- the plan -------------------------------------------------------------------------

// The concrete action for each step on `platform`: { step, skip?, notRun?, argv?, env, bash? }.
export function plan(tier, { platform, quick, work, only = null, runId = process.pid }) {
  const steps = TIERS[tier];
  // --only keeps the loopback sandbox steps whenever it keeps a sandboxed step: on Linux a
  // sandboxed step never runs without them.
  const keepSandbox = only && steps.some((s) => s.sandbox && only.includes(s.id));
  return steps
    // ...and always the guard steps: an --only run is still refused on a foreign cargo config
    // or an inherited variable that changes the build.
    .filter((s) => !only || only.includes(s.id) || GUARDS.has(s.id) || (keepSandbox && s.id.startsWith('loopback-')))
    .map((s) => {
      if (s.os && !s.os.includes(platform)) return { step: s, [s.notRunElsewhere ? 'notRun' : 'skip']: s.osReason };
      const env = { ...(s.offline ? OFFLINE : {}), ...(s.env ?? {}), ...(quick && s.quickEnv ? s.quickEnv : {}) };
      if (s.check) return { step: s, check: s.check, env };
      if (s.bash) return { step: s, bash: s.bash, env: s.bash === 'g3-macos' ? { ...env, G3_WORK: work } : env };
      let argv = s.cmd.map((a) => a.replace('<work>', work));
      // A short work dir on Windows: a deep checkout plus target/<copy>/target/... overruns
      // MAX_PATH (PR #365 review).
      // One per run: a fixed %TEMP%\oac-cts was shared by overlapping runs, which then failed
      // ("could not find Cargo.toml", EPERM removing the copy).
      if (s.winWorkDir && platform === 'win32') {
        const workDir = join(tmpdir(), `${s.winWorkDir}-${runId}`);
        return { step: s, argv: [...argv, '--work-dir', workDir], env, workDir };
      }
      if (s.sandbox && platform === 'linux') argv = ['bash', 'scripts/loopback-only.sh', ...argv];
      return { step: s, argv, env };
    });
}

// ---- running --------------------------------------------------------------------------

// A section of .github/local-ci.sh, run as `bash <file> <section>` from a temporary copy with
// CRs dropped (a Windows checkout may hold CRLF): bash reads the body from that file, never
// from stdin, so a child that reads stdin (python, openssl) cannot consume the rest of the
// script (PR #365 review). (`bash -c <body>` was tried first: Git Bash's command-line
// parsing on Windows mangled the 10 KB body.) Shell options as GitHub Actions ran a `run:`
// step: --noprofile --norc -eo pipefail.
const bashFiles = new Map();
function bashFile(bodyText) {
  const text = bodyText.replace(/\r/g, '');
  if (!bashFiles.has(text)) {
    const dir = mkdtempSync(join(tmpdir(), 'oac-local-ci-sh-'));
    const file = join(dir, 'local-ci.sh');
    writeFileSync(file, text);
    bashFiles.set(text, file);
  }
  return bashFiles.get(text);
}
// Best-effort cleanup on normal JS exit. Forced termination (including Windows
// child.kill('SIGINT')), a crash, or a locked child file can leave a directory behind.
// Console cancellation is not proven by the normal-exit self-test. The next run with
// that PID clears its stale directory before the step; other PID directories are untouched.
const workDirs = new Set();
process.on('exit', () => {
  for (const f of bashFiles.values()) rmSync(dirname(f), { recursive: true, force: true });
  for (const d of workDirs) {
    try {
      rmSync(d, { recursive: true, force: true });
    } catch {}
  }
});
export function bashArgs(section, bodyText = readFileSync(join(repoRoot, BASH_BODIES), 'utf8')) {
  return ['--noprofile', '--norc', '-eo', 'pipefail', bashFile(bodyText), section];
}

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
    const env = childEnv(process.env, a.env);
    if (a.check === 'environment') {
      const v = envVerdict(process.env, ctx.allowEnv);
      done(v.status, v.note);
      continue;
    }
    if (a.check === 'toolchain') {
      const r = spawnSync('rustc', ['--version'], { cwd: repoRoot, encoding: 'utf8', env });
      const out = (r.stdout ?? '').trim();
      console.log(out || r.error?.message);
      done(r.status === 0 && out.startsWith(`rustc ${TOOLCHAIN} `) ? 'PASS' : 'FAIL', out || 'rustc not found');
      continue;
    }
    if (a.check === 'cargo-config') {
      const files = allCargoConfigs();
      ctx.cargoConfigs = files;
      for (const f of files) console.log(`${f.path}: ${f.flagged.length ? `holds ${f.flagged.join(', ')} (information only)` : 'no key recognised (information only)'}`);
      const v = cargoConfigVerdict(files, ctx.allowCargoConfig);
      done(v.status, v.note);
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
    // A run's own work dir (winWorkDir): cleared first (a pid reused after a crashed run) and
    // removed afterwards, also if the step fails.
    const clearWorkDir = () => {
      if (!a.workDir) return;
      try {
        rmSync(a.workDir, { recursive: true, force: true, maxRetries: 3 });
      } catch (e) {
        console.log(`local-ci: could not remove ${a.workDir}: ${e.message}`);
      }
    };
    clearWorkDir();
    if (a.workDir) workDirs.add(a.workDir);
    let r;
    if (a.bash) {
      r = spawnSync(ctx.bash, bashArgs(a.bash), { cwd: repoRoot, env, stdio: ['ignore', 'inherit', 'inherit'] });
    } else {
      const [prog, ...args] = a.argv;
      const program = prog === 'node' ? process.execPath : prog === 'bash' ? ctx.bash : prog;
      const nodeArgs = args.map((x) => (x === 'node' ? process.execPath : x));
      r = spawnSync(program, nodeArgs, { cwd: repoRoot, env, stdio: 'inherit' });
    }
    clearWorkDir();
    workDirs.delete(a.workDir);
    if (r.error) done('FAIL', r.error.message);
    else if (a.step.notRunElsewhere && r.status === 3) done('NOT RUN', 'the script reported it cannot run here');
    else done(r.status === 0 ? 'PASS' : 'FAIL', r.status === 0 ? '' : `exit ${r.status ?? r.signal}`);
  }
  return results;
}

const fmt = (ms) => (ms < 60000 ? `${(ms / 1000).toFixed(1)}s` : `${Math.floor(ms / 60000)}m${String(Math.round((ms % 60000) / 1000)).padStart(2, '0')}s`);

function cargoConfigLine(ctx) {
  const files = ctx.cargoConfigs ?? allCargoConfigs();
  if (!files.length) return 'none';
  const list = files.map((f) => `**\`${f.path}\`**${f.local ? ' (untracked, in the repository)' : ''}${f.flagged.length ? ` (holds ${f.flagged.join(', ')})` : ''}`).join('; ');
  return `${list}${ctx.allowCargoConfig ? ' (--allow-cargo-config)' : ''}`;
}

function summary(tier, results, ctx, totalMs) {
  const head = git(['rev-parse', 'HEAD']) ?? '(unknown)';
  const branch = git(['rev-parse', '--abbrev-ref', 'HEAD']) ?? '?';
  // Untracked files count too (PR #365 re-review B4): an untracked file can change a step.
  // Ignored files (target/ and the like) do not; step cargo-config covers an ignored
  // .cargo/config.
  const dirty = (git(['status', '--porcelain', '--untracked-files=all']) ?? '').split('\n').filter(Boolean).length;
  const n = (s) => results.filter((r) => r.status === s).length;
  const verdict = n('FAIL') ? 'FAIL' : n('NOT RUN') ? 'INCOMPLETE' : 'PASS';
  const rustc = (spawnSync('rustc', ['--version'], { encoding: 'utf8' }).stdout ?? '').trim() || 'rustc not found';
  const er = envReport(process.env);
  const envLine = [
    ...er.switches.map((k) => `**${k}** (refused)`),
    ...er.build.map((k) => `**${k}**${ctx.allowEnv ? ' (--allow-env)' : ' (refused)'}`),
    ...er.listed,
  ].join(', ') || 'none';
  const lines = [
    `### local-ci: ${verdict} (tier ${tier}${ctx.quick ? ', --quick' : ''}${ctx.only ? ', PARTIAL --only' : ''})`,
    '',
    `- HEAD: \`${head}\` (${branch}); work tree ${dirty ? `DIRTY (${dirty} file(s) changed or untracked): this is not a run of HEAD` : 'clean'}`,
    `- Platform: ${ctx.platform} ${process.arch}${ctx.wsl ? ' (WSL)' : ''}, OS ${osRelease()}; node ${process.version}; ${rustc}`,
    `- Loopback-only sandbox: ${ctx.platform === 'linux' ? 'used for the sandboxed steps' : 'not available on this OS (as on the old windows/macos CI legs)'}`,
    `- Inherited environment that switches a step off or changes the build (OAC_HERDR_SELFTEST_*, OAC_TEST_*; every CARGO_* but TERM_/HTTP_/NET_/LOG/INCREMENTAL, CARGO, NODE_OPTIONS, RUSTFLAGS, RUSTDOCFLAGS, RUSTC(_WRAPPER/_BOOTSTRAP), a RUSTUP_TOOLCHAIN off the pin; CARGO_HOME listed): ${envLine}`,
    `- Cargo config files outside the repository or untracked in it (any one fails unless --allow-cargo-config): ${cargoConfigLine(ctx)}`,
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

  // 1. the port contract (PR #365 review B1), frozen in scripts/local-ci.ported.json
  let ported = null;
  try {
    ported = loadPorted();
  } catch (e) {
    check(`contract: ${PORTED_FILE} reads as JSON`, false, e.message);
  }
  if (ported) {
    const live = portViolations(TIERS, ported);
    check(`contract: the live plan matches all ${ported.rows?.length} frozen rows exactly, both ways, with count ${ported.count} and sha256 ${String(ported.sha256).slice(0, 12)}...`, live.length === 0, live.join('; '));
    for (const r of ported.rows ?? []) check(`contract: ${r.from} -> ${r.tier}/${r.id}`, TIERS[r.tier]?.some((s) => s.id === r.id));
    for (const [name, mutate] of PORT_MUTATIONS) {
      const t = structuredClone(TIERS);
      mutate(t);
      const v = portViolations(t, ported);
      check(`contract: planted "${name}" is caught`, v.length > 0);
    }
    for (const [name, mutate] of TABLE_MUTATIONS) {
      const p = structuredClone(ported);
      mutate(p);
      check(`contract: frozen-table edit "${name}" is caught`, portViolations(TIERS, p).length > 0);
    }
    check('contract: the review\'s "remove the step, its row and its paths entry" is caught (the row lives in another file)', (() => {
      const t = structuredClone(TIERS);
      t.default = t.default.filter((s) => s.id !== 'crate-deps-adapters-alone');
      return portViolations(t, ported).some((x) => x.includes('crate-deps-adapters-alone') && x.includes('dropped'));
    })());
    // Tiers of this runner's own (LOCAL_TIERS) carry no rows; a ported tier can't become one.
    check(`contract: the runner's own tiers (${[...LOCAL_TIERS].join(', ')}) have no ported rows and are not the default`, !LOCAL_TIERS.has('default') && !(ported.rows ?? []).some((r) => LOCAL_TIERS.has(r.tier)) && [...LOCAL_TIERS].every((t) => TIERS[t]));
    check('contract: planted "mark a ported tier (keystore) as the runner\'s own" is caught', portViolations(TIERS, ported, new Set([...LOCAL_TIERS, 'keystore'])).some((x) => x.startsWith('keystore/')));
    check('contract: planted "mark the default tier as the runner\'s own" is caught', portViolations(TIERS, ported, new Set([...LOCAL_TIERS, 'default'])).length > 0);
    check('contract: planted "move cargo test from the default tier to the lan tier" is caught', (() => {
      const t = structuredClone(TIERS);
      t.lan.push(t.default.find((s) => s.id === 'cargo-test'));
      t.default = t.default.filter((s) => s.id !== 'cargo-test');
      return portViolations(t, ported).some((x) => x.startsWith('default/cargo-test') && x.includes('dropped'));
    })());
  }
  for (const [tier, steps] of Object.entries(TIERS)) {
    const ids = steps.map((s) => s.id);
    check(`coverage: ${tier} step ids are unique`, new Set(ids).size === ids.length, ids.join(','));
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
  check('plan: --only a sandboxed step keeps the guards and the loopback select and probe steps', JSON.stringify(plan('default', { platform: 'linux', work: '/w', only: ['herdr-self-test'] }).map((a) => a.step.id)) === '["cargo-config","environment","loopback-select","loopback-probe","herdr-self-test"]');
  check('plan: --only an unsandboxed step runs it with the guards only', JSON.stringify(plan('default', { platform: 'linux', work: '/w', only: ['skills'] }).map((a) => a.step.id)) === '["cargo-config","environment","skills"]');
  check('plan: g3-macos is NOT RUN off macOS and runs on macOS', plan('g3-macos', { platform: 'linux', work: '/w' })[0].notRun && plan('g3-macos', { platform: 'darwin', work: '/w' })[0].bash === 'g3-macos');
  // Overlapping runs never share a Windows work dir (a fixed %TEMP%\oac-cts did).
  {
    const cts = (runId, platform = 'win32') => plan('default', { platform, work: '/w', runId }).find((a) => a.step.id === 'compiled-tests-self-test');
    const a = cts(111);
    const want = join(tmpdir(), 'oac-cts-111');
    check('plan: on Windows compiled-tests-self-test gets a work dir of its run\'s own (oac-cts-<pid>)', a.workDir === want && JSON.stringify(a.argv.slice(-2)) === JSON.stringify(['--work-dir', want]));
    check('plan: two overlapping runs get two different work dirs', cts(111).workDir !== cts(222).workDir);
    check('plan: the run id defaults to this process\'s pid', plan('default', { platform: 'win32', work: '/w' }).find((x) => x.step.id === 'compiled-tests-self-test').workDir === join(tmpdir(), `oac-cts-${process.pid}`));
    check('plan: off Windows there is no work dir', ['linux', 'darwin'].every((p) => !cts(111, p).workDir && !cts(111, p).argv.includes('--work-dir')));
    check('plan: no step of any tier passes a fixed directory under the temp dir on Windows', Object.keys(TIERS).every((t) => plan(t, { platform: 'win32', work: '/w', runId: 333 }).every((x) => (x.argv ?? []).every((arg) => !arg.startsWith(tmpdir()) || arg.endsWith('-333')))));
    // The runner clears a stale work dir before the step and removes it after.
    const dir = join(tmpdir(), `oac-local-ci-wd-${process.pid}`);
    mkdirSync(dir, { recursive: true });
    writeFileSync(join(dir, 'stale'), 'from a crashed run');
    const script = "const fs=require('fs');const d=process.argv[1];if(fs.existsSync(d+'/stale'))process.exit(1);fs.mkdirSync(d,{recursive:true});fs.writeFileSync(d+'/f','x')";
    const [res] = run([{ step: { id: 'work-dir-probe', name: 'work dir probe' }, argv: ['node', '-e', script, dir], env: {}, workDir: dir }], { platform: 'win32', bash: null });
    check('run: a run\'s work dir is cleared before its step and removed after it', res.status === 'PASS' && !existsSync(dir), `${res.status} exists=${existsSync(dir)}`);
    rmSync(dir, { recursive: true, force: true });
  }
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
  check('policy: the lan tier is an opt-in (its OAC_TEST_LAN would fail the default-tier policy)', policy(TIERS.lan).some((x) => x.includes('D2')));
  check('policy: an OAC_TEST_LAN step planted in the default tier is caught', plant({ ...TIERS.lan.at(-1), id: 'x' }));

  // 5. planted boundary-lint checks in throwaway git trees
  const bash = findBash();
  check('boundary: bash found (Git Bash on Windows)', Boolean(bash));
  check('boundary: rg on PATH', onPath('rg'));
  check('boundary: a section runs from a CR-free script file with its name as $1, never from stdin', (() => {
    const a = bashArgs('boundary-check-3', 'x\r\ny');
    return !a.includes('-s') && !a.includes('-c') && readFileSync(a.at(-2), 'utf8') === 'x\ny' && a.at(-1) === 'boundary-check-3';
  })());
  if (bash) {
    // A child reading stdin must not eat the script: what it reads is stdin, and the next
    // line of the section still runs.
    const r = spawnSync(bash, bashArgs('s', 'read -r line; echo "got-$line"\necho after-$1\n'), { encoding: 'utf8', input: 'from-stdin\necho injected\n' });
    check('boundary: a child reading stdin does not consume the rest of a section', r.status === 0 && r.stdout.trim() === 'got-from-stdin\nafter-s', `${r.status} ${JSON.stringify(r.stdout)} ${r.stderr}`);
  }
  if (bash && onPath('rg')) {
    for (const [section, name, files, want] of BOUNDARY_CASES) {
      const dir = mkdtempSync(join(tmpdir(), 'oac-local-ci-'));
      try {
        for (const [rel, text] of Object.entries(files)) {
          mkdirSync(dirname(join(dir, rel)), { recursive: true });
          writeFileSync(join(dir, rel), text);
        }
        spawnSync('git', ['init', '-q'], { cwd: dir });
        spawnSync('git', ['add', '-A'], { cwd: dir });
        const r = spawnSync(bash, bashArgs(section, bashBodies), { cwd: dir, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
        check(`boundary: ${section}: ${name} exits ${want}`, r.status === want, `exit ${r.status}: ${(r.stdout + r.stderr).slice(-300)}`);
      } finally {
        rmSync(dir, { recursive: true, force: true });
      }
    }
  }

  // 6. cargo config outside the repository (PR #365 review B2), in a throwaway tree:
  //    <t>/a/.cargo/config.toml ([patch]), <t>/a/b/.cargo/config (plain), repo <t>/a/b/repo,
  //    CARGO_HOME <t>/home/config.toml (replace-with). Files above <t> are the machine's own
  //    and are not asserted on.
  {
    const t = mkdtempSync(join(tmpdir(), 'oac-cargo-cfg-'));
    try {
      const put = (rel, text) => {
        mkdirSync(dirname(join(t, rel)), { recursive: true });
        writeFileSync(join(t, rel), text);
      };
      put('a/.cargo/config.toml', '[patch.crates-io]\ntokio = { path = "../tokio" }\n');
      put('a/b/.cargo/config', '[build]\njobs = 2\n# [source.crates-io] in a comment is not a key\n');
      put('a/b/repo/.cargo/config.toml', '[patch.crates-io]\nx = { path = "y" }\n');
      put('home/config.toml', "[source.crates-io]\nreplace-with = 'mirror'\n");
      put('home2/config', "paths = ['../x']\n");
      const mine = (files) => files.filter((f) => f.path.startsWith(t));
      const found = mine(cargoConfigFiles({ root: join(t, 'a', 'b', 'repo'), cargoHome: join(t, 'home') }));
      const at = (rel) => found.find((f) => f.path === join(t, rel));
      check('cargo config: an ancestor .cargo/config.toml with [patch] is found and flagged', at('a/.cargo/config.toml')?.flagged.includes('[patch]'));
      check('cargo config: a plain ancestor .cargo/config (comment ignored) is found, not flagged', at('a/b/.cargo/config')?.flagged.length === 0);
      check('cargo config: $CARGO_HOME/config.toml with [source] replace-with is found and flagged', ['[source]', 'replace-with'].every((k) => at('home/config.toml')?.flagged.includes(k)));
      check("cargo config: the repository's own .cargo/ is left to check-crate-deps rule 7", !at('a/b/repo/.cargo/config.toml'));
      const home2 = mine(cargoConfigFiles({ root: join(t, 'a', 'b', 'repo'), cargoHome: join(t, 'home2') })).find((f) => f.path === join(t, 'home2', 'config'));
      check('cargo config: a legacy $CARGO_HOME/config with paths is found and flagged', home2?.flagged.includes('paths'));
      check('cargo config: a plain file with no recognised key still fails the step without --allow-cargo-config', cargoConfigVerdict([at('a/b/.cargo/config')], false).status === 'FAIL');
      check('cargo config: --allow-cargo-config passes and still names the files', (() => {
        const v = cargoConfigVerdict(found, true);
        return v.status === 'PASS' && v.note.includes(join(t, 'home', 'config.toml'));
      })());
      check('cargo config: none found passes', cargoConfigVerdict([], false).status === 'PASS');
      // The re-review's spellings (and the one it proved cargo honours): each, as the only
      // $CARGO_HOME/config.toml, must fail the step without the flag, whatever the flags say.
      const SPELLINGS = [
        '["source".crates-io]\n"replace-with" = "vend"\n["source".vend]\ndirectory = "no-such-vendor-dir"\n',
        "['source'.v]\ndirectory = 'x'\n",
        'source = { crates-io = { "replace-with" = "v" }, v = { directory = "x" } }\n',
        'patch = { crates-io = { serde = { path = "x" } } }\n',
        '"patch".crates-io.serde.path = "x"\n',
        '["patch".crates-io]\nserde = { path = "x" }\n',
        '"paths" = ["../x"]\n',
        '[ source . crates-io ]\nreplace-with = "v"\n',
        '[build]\njobs = 1\n',
      ];
      SPELLINGS.forEach((text, i) => {
        const home = join(t, `spell${i}`);
        put(`spell${i}/config.toml`, text);
        const f = mine(cargoConfigFiles({ root: join(t, 'a', 'b', 'repo'), cargoHome: home })).filter((x) => x.path.startsWith(home));
        check(`cargo config: spelling ${i + 1} ${JSON.stringify(text.split('\n')[0])} is found and fails without --allow-cargo-config`, f.length === 1 && cargoConfigVerdict(f, false).status === 'FAIL');
      });
    } finally {
      rmSync(t, { recursive: true, force: true });
    }
  }

  // 6b. repository-local untracked cargo config (PR #365 re-review B4), in a throwaway git repo
  {
    const r = mkdtempSync(join(tmpdir(), 'oac-cargo-local-'));
    try {
      const put = (rel, text) => {
        mkdirSync(dirname(join(r, rel)), { recursive: true });
        writeFileSync(join(r, rel), text);
      };
      spawnSync('git', ['init', '-q'], { cwd: r });
      put('.cargo/config.toml', '[build]\njobs = 2\n');
      spawnSync('git', ['add', '.cargo/config.toml'], { cwd: r });
      const rel = (f) => f.path.slice(r.length + 1).split('\\').join('/');
      check('repo cargo config: a tracked root .cargo/config.toml is left to check-crate-deps rule 7', repoLocalUntrackedConfigs(r).length === 0);
      put('adapters/x/.cargo/config.toml', '[alias]\nclippy = "test --no-run"\n');
      put('cli/.cargo/config', 'alias.clippy = "test --no-run"\n');
      put('target/debug/.cargo/config.toml', '[alias]\nclippy = "x"\n');
      const found = repoLocalUntrackedConfigs(r);
      check("repo cargo config: the review's untracked member [alias] clippy = \"test --no-run\" is found and fails", found.some((f) => rel(f) === 'adapters/x/.cargo/config.toml' && f.flagged.includes('[alias]')) && cargoConfigVerdict(found, false).status === 'FAIL');
      check('repo cargo config: an untracked legacy member .cargo/config with a dotted alias is found', found.some((f) => rel(f) === 'cli/.cargo/config' && f.flagged.includes('alias.*')));
      check('repo cargo config: target/ is not searched', !found.some((f) => rel(f).startsWith('target/')));
      rmSync(join(r, '.cargo'), { recursive: true, force: true });
      spawnSync('git', ['rm', '-q', '--cached', '.cargo/config.toml'], { cwd: r });
      put('.cargo/config.toml', '[alias]\nclippy = "test --no-run"\n');
      const root = repoLocalUntrackedConfigs(r).find((f) => rel(f) === '.cargo/config.toml');
      check("repo cargo config: the review's untracked root .cargo/config.toml with [alias] is found and fails without --allow-cargo-config", root?.local === true && cargoConfigVerdict([root], false).status === 'FAIL' && cargoConfigVerdict([root], true).status === 'PASS');
    } finally {
      rmSync(r, { recursive: true, force: true });
    }
  }

  // 6c. a repository `.cargo` that is a link (PR #365 re-review B5): a junction (Windows; a
  //     plain directory symlink elsewhere, where Node ignores the junction type) and a
  //     directory symlink are followed, and a dangling one fails closed. A link type the OS
  //     will not let this user create (a Windows symlink without Developer Mode) is skipped
  //     and says so.
  {
    const base = mkdtempSync(join(tmpdir(), 'oac-cargo-link-'));
    try {
      const shared = join(base, 'shared-dot-cargo');
      mkdirSync(shared, { recursive: true });
      writeFileSync(join(shared, 'config.toml'), '[alias]\nclippy = "test --no-run"\n');
      const linked = (name, type, target = shared, at = '.cargo') => {
        const r = join(base, name);
        mkdirSync(join(r, 'adapters', 'x'), { recursive: true });
        spawnSync('git', ['init', '-q'], { cwd: r });
        try {
          symlinkSync(target, join(r, at), type);
        } catch (e) {
          return { r, skip: e.code ?? String(e) };
        }
        return { r };
      };
      const cases = [
        ['junction', process.platform === 'win32' ? 'junction' : 'dir', 'a root .cargo junction (directory symlink off Windows)'],
        ['symlink', 'dir', 'a root .cargo directory symlink'],
      ];
      for (const [name, type, what] of cases) {
        const { r, skip } = linked(name, type);
        if (skip) {
          console.log(`skip repo cargo config: ${what}: this OS refused to create it (${skip})`);
          continue;
        }
        const f = repoLocalUntrackedConfigs(r);
        check(`repo cargo config: ${what} to a [alias] clippy config is followed, found and fails`, f.some((x) => x.flagged.includes('[alias]')) && cargoConfigVerdict(f, false).status === 'FAIL');
      }
      const member = linked('member', process.platform === 'win32' ? 'junction' : 'dir', shared, join('adapters', 'x', '.cargo'));
      if (!member.skip) {
        const f = repoLocalUntrackedConfigs(member.r);
        check('repo cargo config: a member .cargo link is followed too', f.some((x) => x.flagged.includes('[alias]')));
      }
      const dangling = linked('dangling', process.platform === 'win32' ? 'junction' : 'dir', join(base, 'no-such-dir'));
      if (dangling.skip) console.log(`skip repo cargo config: a dangling .cargo link: this OS refused to create it (${dangling.skip})`);
      else {
        const f = repoLocalUntrackedConfigs(dangling.r);
        check('repo cargo config: a dangling .cargo link fails closed', f.length === 1 && f[0].flagged[0].includes('cannot be resolved') && cargoConfigVerdict(f, false).status === 'FAIL');
      }
    } finally {
      rmSync(base, { recursive: true, force: true });
    }
  }

  // 7. inherited environment (PR #365 re-review B3)
  {
    const base = { PATH: '/bin', HOME: '/h' };
    check('environment: a clean environment passes', envVerdict(base, false).status === 'PASS');
    for (const k of ['OAC_HERDR_SELFTEST_UNIT_ONLY', 'OAC_HERDR_SELFTEST_ONLY', 'OAC_TEST_REAL_KEYRING', 'OAC_TEST_LAN', 'oac_test_x']) {
      check(`environment: inherited ${k} fails, --allow-env or not`, envVerdict({ ...base, [k]: '1' }, false).status === 'FAIL' && envVerdict({ ...base, [k]: '1' }, true).status === 'FAIL');
    }
    for (const k of ['NODE_OPTIONS', 'RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'RUSTDOCFLAGS', 'RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER', 'CARGO_BUILD_TARGET', 'CARGO_TARGET_DIR', 'CARGO_PROFILE_DEV_OPT_LEVEL', 'CARGO_SOURCE_CRATES_IO_REPLACE_WITH', 'CARGO_PATCH_X']) {
      check(`environment: inherited ${k} fails without --allow-env and passes, listed, with it`, envVerdict({ ...base, [k]: 'x' }, false).status === 'FAIL' && envVerdict({ ...base, [k]: 'x' }, true).note.includes(k));
    }
    check('environment: CARGO_HOME is listed, not refused', (() => {
      const v = envVerdict({ ...base, CARGO_HOME: '/c' }, false);
      return v.status === 'PASS' && v.note.includes('CARGO_HOME');
    })());
    check('environment: CARGO_TERM_COLOR and CARGO_NET_OFFLINE are not caught (controls)', envVerdict({ ...base, CARGO_TERM_COLOR: 'always', CARGO_NET_OFFLINE: 'true' }, false).status === 'PASS');
    // PR #365 re-review B4: CARGO_* is an allowlist; every config key has an env form.
    for (const k of ['CARGO_ALIAS_CLIPPY', 'CARGO_ALIAS_TEST', 'CARGO_UNSTABLE_BUILD_STD', 'CARGO_REGISTRIES_MIRROR_INDEX', 'CARGO_REGISTRY_DEFAULT', 'CARGO_INSTALL_ROOT', 'CARGO', 'RUSTC_BOOTSTRAP', 'RUSTDOC', 'cargo_alias_clippy']) {
      check(`environment: inherited ${k} fails without --allow-env (B4)`, envVerdict({ ...base, [k]: 'test --no-run' }, false, '1.98.1').status === 'FAIL' && envVerdict({ ...base, [k]: 'x' }, true, '1.98.1').status === 'PASS');
    }
    check('environment: the review\'s CARGO_ALIAS_CLIPPY="test --no-run" names the variable', envVerdict({ ...base, CARGO_ALIAS_CLIPPY: 'test --no-run' }, false, '1.98.1').note.includes('CARGO_ALIAS_CLIPPY'));
    for (const [k, v] of [['CARGO_TERM_VERBOSE', 'true'], ['CARGO_HTTP_TIMEOUT', '30'], ['CARGO_NET_RETRY', '3'], ['CARGO_LOG', 'info'], ['CARGO_INCREMENTAL', '1']]) {
      check(`environment: allowlisted ${k} passes (control)`, envVerdict({ ...base, [k]: v }, false, '1.98.1').status === 'PASS');
    }
    check('environment: RUSTUP_TOOLCHAIN off the pin fails', envVerdict({ ...base, RUSTUP_TOOLCHAIN: 'nightly' }, false, '1.98.1').status === 'FAIL');
    check('environment: RUSTUP_TOOLCHAIN at the pin, bare or with a host triple, passes (control)', ['1.98.1', '1.98.1-x86_64-pc-windows-msvc'].every((v) => envVerdict({ ...base, RUSTUP_TOOLCHAIN: v }, false, '1.98.1').status === 'PASS'));
    check('environment: the pin is read from rust-toolchain.toml', toolchainChannel() === TOOLCHAIN);
    const child = childEnv({ ...base, OAC_HERDR_SELFTEST_ONLY: 'x', OAC_HERDR_SELFTEST_UNIT_ONLY: '1', OAC_TEST_REAL_KEYRING: '1' }, {});
    check('environment: inherited switches never reach a child', !Object.keys(child).some((k) => /^OAC_/.test(k)));
    const quick = plan('default', { platform: 'linux', quick: true, work: '/w' }).find((a) => a.step.id === 'herdr-self-test');
    check('environment: --quick sets its herdr switch for that child only', childEnv(base, quick.env).OAC_HERDR_SELFTEST_UNIT_ONLY === '1' && !('OAC_HERDR_SELFTEST_UNIT_ONLY' in childEnv(base, plan('default', { platform: 'linux', quick: true, work: '/w' }).find((a) => a.step.id === 'cargo-test').env)));
    const ks = plan('keystore', { platform: 'win32', work: '/w' }).find((a) => a.step.id === 'keystore-os-store');
    check("environment: the keystore tier's own OAC_TEST_REAL_KEYRING reaches its child", childEnv({ ...base, OAC_TEST_REAL_KEYRING: '0' }, ks.env).OAC_TEST_REAL_KEYRING === '1');
    for (const platform of ['linux', 'win32', 'darwin']) {
      const lan = plan('lan', { platform, work: '/w' });
      const step = lan.find((a) => a.step.id === 'lan-zenoh-local-mode');
      check(`environment: the lan tier on ${platform} sets OAC_TEST_LAN=1 for its test child only, outside the loopback wrapper`, childEnv({ ...base, OAC_TEST_LAN: '0' }, step.env).OAC_TEST_LAN === '1' && step.argv[0] === 'cargo' && lan.filter((a) => a !== step).every((a) => !('OAC_TEST_LAN' in (a.env ?? {}))));
    }
    check('environment: no default-tier step sets OAC_TEST_LAN, full or --quick', [false, true].every((quick) => plan('default', { platform: 'win32', quick, work: '/w' }).every((a) => !('OAC_TEST_LAN' in (a.env ?? {})))));
    check('environment: every tier that builds runs the environment and cargo-config steps first', ['default', 'keystore', 'scale', 'mutation', 'lan'].every((t) => JSON.stringify(TIERS[t].slice(0, 3).map((s) => s.id)) === '["toolchain","cargo-config","environment"]'));
  }

  console.log(`\nlocal-ci self-test: ${failed ? `${failed} FAILED` : 'all passed'}`);
  return failed ? 1 : 0;
}

// ---- main -----------------------------------------------------------------------------

function usage(msg) {
  if (msg) console.error(`local-ci: ${msg}`);
  console.error('usage: node scripts/local-ci.mjs [--quick] [--allow-cargo-config] [--allow-env] [--tier default|keystore|scale|mutation|g3-macos|lan] [--only id,...] [--list] | --self-test');
  process.exit(2);
}

function main(argv) {
  let tier = 'default';
  let quick = false;
  let list = false;
  let only = null;
  let allowCargoConfig = false;
  let allowEnv = false;
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--self-test') {
      if (argv.length !== 1) usage('--self-test takes no other option');
      return selfTest();
    } else if (a === '--quick') quick = true;
    else if (a === '--allow-cargo-config') allowCargoConfig = true;
    else if (a === '--allow-env') allowEnv = true;
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
  const ctx = { platform, quick, only, bash, wsl, allowCargoConfig, allowEnv };
  const t0 = Date.now();
  const results = run(plan(tier, { platform, quick, work, only }), ctx);
  const { text, verdict } = summary(tier, results, ctx, Date.now() - t0);
  writeFileSync(join(work, `summary-${tier}.md`), `${text}\n`);
  console.log(`\n${text}\n\n(also written to target/local-ci/summary-${tier}.md)`);
  return verdict === 'PASS' ? 0 : verdict === 'FAIL' ? 1 : 3;
}

if (process.argv[1] && resolve(process.argv[1]) === scriptPath) process.exit(main(process.argv.slice(2)));
