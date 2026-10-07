#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
//
// Runtime cross-check of the security suite's threat map (#60, PR #327 re-review B1).
//
// tests/threat_map.rs checks THREATS against the parsed sources, but what a parser sees is
// not always what cargo compiles: a `#[cfg]` can compile a proof out, and a `#[path]`
// module, a `tests/<dir>/main.rs` target, a macro-generated test or a test attribute
// spelled another way can add one the parser never sees. This script asks cargo instead:
//
//   1. builds every test target of `oac-security-suite` (`cargo test --no-run`) and reads
//      the executables from cargo's JSON messages;
//   2. lists each executable's tests (`--list`) and its ignored tests (`--list --ignored`);
//   3. reads the map from the suite's own `threat-map` binary;
//   4. fails when a mapped proof or fact is not compiled exactly once, or is ignored; when a
//      gated placeholder is not compiled exactly once as ignored; when a compiled test is in
//      no row (the map checks of `tests/threat_map.rs` excepted, by name); and when a gated
//      placeholder passes when it is run (`--ignored`), since it must never be a pass;
//   5. fails on a test no listing sees (#329): a target with doc-tests on (`cargo metadata`;
//      the suite sets `[lib] doctest = false`), and a test executable whose `--list` does
//      not end in libtest's `N tests, M benchmarks` line (a `harness = false` target runs its
//      own `main` and lists nothing).
//
//   node tests/security/check-compiled-tests.mjs              # check this workspace
//   node tests/security/check-compiled-tests.mjs --self-test [--work-dir <dir>]
//        # plant each known evasion in a copy and check it is caught; the copy and its
//        # target/ live under <dir> (default: <repo>/target/security-compiled-tests), so a
//        # short <dir> keeps Windows paths under the length limit (#329)
//
// Node built-ins only. Exit codes: 0 = clean; 1 = violation (or a failed self-test);
// 2 = usage or environment error.

import { spawnSync } from 'node:child_process';
import { mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { copyWorkspace, workDirProblem } from './mutation-check.mjs';

const scriptPath = fileURLToPath(import.meta.url);
const repoRoot = resolve(dirname(scriptPath), '..', '..');
const PKG = 'oac-security-suite';

// The tests of tests/threat_map.rs: checks of the map itself, not threat proofs.
const MAP_CHECKS = new Set([
  'no_test_is_hidden_from_the_parser',
  'every_06_row_is_mapped_once',
  'every_spec_13_row_is_mapped',
  'every_mapped_test_exists_and_every_test_is_mapped',
  'every_test_name_starts_with_a_row',
  'every_cited_core_test_exists',
  'statuses_match_what_runs',
  'the_09_table_is_the_rendered_map',
  'no_test_target_escapes_the_listing',
]);

// The last line libtest's `--list` prints (`--format pretty`, the default).
const LIST_SUMMARY = /^\d+ tests?, \d+ benchmarks?\s*$/m;

function run(cmd, args, cwd, env = {}) {
  const r = spawnSync(cmd, args, {
    cwd,
    encoding: 'utf8',
    env: { ...process.env, ...env },
    maxBuffer: 256 * 1024 * 1024,
  });
  if (r.error) throw r.error;
  return r;
}

function listed(out) {
  return out
    .split(/\r?\n/)
    .map((l) => /^(.+): test$/.exec(l))
    .filter(Boolean)
    .map((m) => m[1]);
}

/** Returns a list of violation strings; throws on an environment error. */
export function check(ws, env = {}) {
  const build = run('cargo', ['test', '-p', PKG, '--locked', '--no-run', '--message-format=json'], ws, env);
  if (build.status !== 0) {
    return [`the suite does not build:\n${build.stderr.split(/\r?\n/).slice(-20).join('\n')}`];
  }
  const targets = [];
  for (const line of build.stdout.split(/\r?\n/)) {
    if (!line.startsWith('{')) continue;
    const m = JSON.parse(line);
    if (m.reason === 'compiler-artifact' && m.profile?.test && m.executable && m.package_id.includes(PKG)) {
      targets.push({ name: m.target.name, kind: m.target.kind.join(','), exe: m.executable });
    }
  }
  if (targets.length === 0) throw new Error('no test executables found');
  const map = run('cargo', ['run', '-q', '-p', PKG, '--locked', '--bin', 'threat-map'], ws, env);
  if (map.status !== 0) return [`the threat-map binary failed:\n${map.stderr}`];
  const rows = JSON.parse(map.stdout);

  const v = [];
  // Doc-tests: neither listing below sees them, so none may exist (#329).
  const meta = run('cargo', ['metadata', '--no-deps', '--format-version', '1', '--locked'], ws, env);
  if (meta.status !== 0) return [`cargo metadata failed:\n${meta.stderr}`];
  const pkg = JSON.parse(meta.stdout).packages.find((p) => p.name === PKG);
  if (!pkg) throw new Error(`${PKG} not in cargo metadata`);
  for (const t of pkg.targets) {
    if (t.doctest) v.push(`${t.name} (${t.kind.join(',')}) has doc-tests on: a doc-test is a test no listing sees; set doctest = false`);
  }

  const all = new Map(); // test name -> [target]
  const ignored = new Map();
  for (const t of targets) {
    const a = run(t.exe, ['--list'], ws);
    const i = run(t.exe, ['--list', '--ignored'], ws);
    if (a.status !== 0 || i.status !== 0) return [...v, `${t.name}: --list failed`];
    // A harness = false target runs its own main: it lists nothing, in no format (#329).
    if (!LIST_SUMMARY.test(a.stdout) || !LIST_SUMMARY.test(i.stdout)) {
      v.push(`${t.name} (${t.kind}): --list is not libtest's (no "N tests, M benchmarks" line); a harness = false target's tests are seen by no check`);
      continue;
    }
    for (const n of listed(a.stdout)) all.set(n, [...(all.get(n) ?? []), `${t.name} (${t.kind})`]);
    for (const n of listed(i.stdout)) ignored.set(n, [...(ignored.get(n) ?? []), `${t.name} (${t.kind})`]);
  }

  const proofs = new Set();
  const gated = new Set();
  const facts = new Set();
  for (const r of rows) {
    for (const n of r.tests) (n.startsWith('gated_') ? gated : proofs).add(n);
    for (const n of r.facts) facts.add(n);
  }
  for (const n of [...proofs, ...facts]) {
    const where = all.get(n) ?? [];
    if (where.length !== 1) v.push(`mapped ${facts.has(n) ? 'fact' : 'proof'} ${n} is compiled ${where.length} time(s), not once${where.length ? `: ${where.join(', ')}` : ' (compiled out, renamed or missing)'}`);
    else if (ignored.has(n)) v.push(`mapped ${facts.has(n) ? 'fact' : 'proof'} ${n} is ignored, so it never runs`);
  }
  for (const n of gated) {
    const where = all.get(n) ?? [];
    if (where.length !== 1) v.push(`gated placeholder ${n} is compiled ${where.length} time(s), not once`);
    else if (!ignored.has(n)) v.push(`gated placeholder ${n} is not ignored`);
  }
  for (const [n, where] of all) {
    const mapChecks = where.every((w) => w.startsWith('threat_map ')) && MAP_CHECKS.has(n);
    if (!proofs.has(n) && !gated.has(n) && !facts.has(n) && !mapChecks) {
      v.push(`compiled test ${n} (${where.join(', ')}) is in no threat row`);
    }
  }
  // A gated placeholder must fail when run.
  for (const t of targets) {
    const r = run(t.exe, ['--ignored'], ws);
    for (const m of r.stdout.matchAll(/^test (\S+) \.\.\. ok$/gm)) {
      v.push(`${t.name}: ignored test ${m[1]} passes when run; a gated placeholder must fail`);
    }
  }
  return v;
}

// --------------------------------------------------------------------------------------
// --self-test: each evasion the PR #327 reviews found, planted alone in a copy.

const at = (anchor, insert) => (t) => {
  if (!t.includes(anchor)) throw new Error(`anchor not found: ${anchor}`);
  return t.replace(anchor, insert + anchor);
};
const replace = (from, to) => (t) => {
  if (!t.includes(from)) throw new Error(`text not found: ${from}`);
  return t.replace(from, to);
};
const append = (s) => (t) => `${t}\n${s}\n`;

// `runtime: true` means this script must catch it, `static: true` that the static checks
// of tests/threat_map.rs must; every plant must be caught by one of them.
const PLANTS = [
  // #329: the PR #327 clean re-review's follow-ups.
  {
    name: '#[cfg_attr(any(), test)] in place of #[test] on a proof',
    runtime: true,
    static: true,
    edits: [['tests/security/tests/replay.rs', replace('#[test]\nfn row04_copy_outside_the_replay_window_is_rejected', '#[cfg_attr(any(), test)]\nfn row04_copy_outside_the_replay_window_is_rejected')]],
  },
  {
    // Compiled out, so no listing sees it: only the static check can.
    name: '#[cfg_attr(any(), test)] on an unmapped function',
    static: true,
    edits: [['tests/security/tests/spoofing.rs', append('#[cfg_attr(any(), test)]\n#[allow(dead_code)]\nfn sneaky_cfg_attr_test() {}')]],
  },
  {
    name: '#[cfg_attr(all(), core::prelude::v1::test)], spelled in full, on an unmapped test',
    runtime: true,
    static: true,
    edits: [['tests/security/tests/spoofing.rs', append('#[cfg_attr(all(), core::prelude::v1::test)]\nfn sneaky_cfg_attr_full_path() {}')]],
  },
  {
    // PR #336 review N3: a raw identifier spells the attribute too.
    name: '#[cfg_attr(all(), r#test)] on an unmapped test',
    runtime: true,
    static: true,
    edits: [['tests/security/tests/spoofing.rs', append('#[cfg_attr(all(), r#test)]\nfn sneaky_cfg_attr_raw_test() {}')]],
  },
  {
    name: '#[cfg_attr(all(), cfg(any()))] compiles a proof out',
    runtime: true,
    static: true,
    edits: [['tests/security/tests/replay.rs', at('#[test]\nfn row04_copy_outside_the_replay_window_is_rejected', '#[cfg_attr(all(), cfg(any()))]\n')]],
  },
  {
    name: 'doc-tests switched on, with a doc-test in src/lib.rs',
    runtime: true,
    static: true,
    edits: [
      ['tests/security/Cargo.toml', replace('doctest = false', 'doctest = true')],
      ['tests/security/src/lib.rs', append('/// ```\n/// assert!(true);\n/// ```\npub fn sneaky_doc_test() {}')],
    ],
  },
  {
    name: 'a harness = false test target with an unmapped test',
    runtime: true,
    static: true,
    files: [['tests/security/harness/custom.rs', 'fn main() {}\n#[test]\nfn sneaky_custom_harness() {}\n']],
    edits: [['tests/security/Cargo.toml', append('[[test]]\nname = "custom"\npath = "harness/custom.rs"\nharness = false')]],
  },
  {
    name: '#[cfg(any())] compiles a proof out',
    runtime: true,
    edits: [['tests/security/tests/replay.rs', at('#[test]\nfn row04_copy_outside_the_replay_window_is_rejected', '#[cfg(any())]\n')]],
  },
  {
    name: '#[cfg(windows)] on a proof (one platform only)',
    runtime: false,
    edits: [['tests/security/tests/replay.rs', at('#[test]\nfn row04_copy_outside_the_replay_window_is_rejected', '#[cfg(windows)]\n')]],
  },
  {
    // An unmapped test compiled out is invisible to cargo too: the static check catches it.
    name: 'a test inside a #[cfg(any())] module',
    runtime: false,
    edits: [['tests/security/tests/replay.rs', append('#[cfg(any())]\nmod gone {\n    #[test]\n    fn row04_hidden_in_a_cfg_module() {}\n}')]],
  },
  {
    name: 'tests/<dir>/main.rs target with an unmapped test',
    runtime: true,
    files: [['tests/security/tests/hidden/main.rs', '#[test]\nfn sneaky_main_rs() {}\n']],
  },
  {
    name: '#[path] module with an unmapped test',
    runtime: true,
    files: [['tests/security/tests/elsewhere/x.rs', '#[test]\nfn sneaky_path_module() {}\n']],
    edits: [['tests/security/tests/spoofing.rs', append('#[path = "elsewhere/x.rs"]\nmod x;')]],
  },
  {
    name: '#[::core::prelude::v1::test] on an unmapped test',
    runtime: true,
    edits: [['tests/security/tests/spoofing.rs', append('#[::core::prelude::v1::test]\nfn sneaky_full_path_attribute() {}')]],
  },
  {
    name: 'a macro_rules!-generated unmapped test',
    runtime: true,
    edits: [['tests/security/tests/spoofing.rs', append('macro_rules! make {\n    ($n:ident) => {\n        #[test]\n        fn $n() {}\n    };\n}\nmake!(sneaky_macro_test);')]],
  },
  {
    name: 'a #[cfg(test)] module in src/lib.rs',
    runtime: true,
    edits: [['tests/security/src/lib.rs', append('#[cfg(test)]\nmod hidden {\n    #[test]\n    fn sneaky_lib_unit_test() {}\n}')]],
  },
  {
    name: 'a mapped proof marked #[ignore]',
    runtime: true,
    edits: [['tests/security/tests/replay.rs', at('fn row04_copy_outside_the_replay_window_is_rejected', '#[ignore]\n')]],
  },
  {
    name: 'a gated body whose panic! is a local no-op macro',
    runtime: true,
    edits: [
      ['tests/security/tests/local_ipc.rs', replace('    std::panic!("GATED on #70: connect as another user', '    panic!("GATED on #70: connect as another user')],
      ['tests/security/tests/local_ipc.rs', at('/// 06 row 13', 'macro_rules! panic {\n    ($($t:tt)*) => {};\n}\n\n')],
    ],
  },
];

function selfTest(work = join(repoRoot, 'target', 'security-compiled-tests')) {
  const ws = join(work, 'ws');
  const env = { CARGO_TARGET_DIR: join(work, 'target') };
  mkdirSync(work, { recursive: true });
  let bad = 0;
  try {
    copyWorkspace(ws);
    const control = check(ws, env);
    const ok = control.length === 0;
    console.log(`${ok ? 'pass' : 'FAIL'}  control: the unmodified copy is clean${ok ? '' : `\n  ${control.join('\n  ')}`}`);
    if (!ok) return 1;
    for (const p of PLANTS) {
      const saved = [];
      try {
        for (const [f, body] of p.files ?? []) {
          const path = join(ws, f);
          mkdirSync(dirname(path), { recursive: true });
          writeFileSync(path, body);
          saved.push([path, null]);
        }
        for (const [f, edit] of p.edits ?? []) {
          const path = join(ws, f);
          const before = readFileSync(path, 'utf8');
          saved.push([path, before]);
          writeFileSync(path, edit(before.replace(/\r\n/g, '\n')));
        }
        const rt = check(ws, env);
        const st = run('cargo', ['test', '-p', PKG, '--locked', '--test', 'threat_map'], ws, env);
        const staticCaught = st.status !== 0;
        const runtimeCaught = rt.length > 0;
        const ok = (runtimeCaught || staticCaught) && (!p.runtime || runtimeCaught) && (!p.static || staticCaught);
        if (!ok) bad++;
        console.log(
          `${ok ? 'pass' : 'FAIL'}  ${p.name}: runtime ${runtimeCaught ? 'caught' : 'missed'}, static ${staticCaught ? 'caught' : 'missed'}` +
            (runtimeCaught ? ` (${rt[0].split('\n')[0]})` : ''),
        );
      } finally {
        for (const [path, before] of saved.reverse()) {
          if (before === null) rmSync(path, { force: true });
          else writeFileSync(path, before);
        }
        rmSync(join(ws, 'tests/security/tests/hidden'), { recursive: true, force: true });
        rmSync(join(ws, 'tests/security/tests/elsewhere'), { recursive: true, force: true });
        rmSync(join(ws, 'tests/security/harness'), { recursive: true, force: true });
      }
    }
  } finally {
    rmSync(ws, { recursive: true, force: true });
  }
  console.log(`self-test: ${PLANTS.length - bad}/${PLANTS.length} evasions caught`);
  return bad ? 1 : 0;
}

const USAGE = 'usage: check-compiled-tests.mjs [--self-test [--work-dir <dir>]]';

function main(argv) {
  if (argv[0] === '--self-test') {
    if (argv.length === 1) return selfTest();
    if (argv.length === 3 && argv[1] === '--work-dir' && argv[2]) {
      const problem = workDirProblem(resolve(argv[2]));
      if (problem) {
        console.error(problem);
        return 2;
      }
      return selfTest(resolve(argv[2]));
    }
    console.error(USAGE);
    return 2;
  }
  if (argv.length) {
    console.error(USAGE);
    return 2;
  }
  const v = check(repoRoot);
  if (v.length === 0) {
    console.log('security suite: every compiled test is mapped, every mapped test compiled and run as mapped: CLEAN');
    return 0;
  }
  for (const x of v) console.log(`FAIL  ${x}`);
  console.log(`security suite threat map vs compiled tests: FAIL -- ${v.length} violation(s)`);
  return 1;
}

if (process.argv[1] && resolve(process.argv[1]) === scriptPath) {
  try {
    process.exitCode = main(process.argv.slice(2));
  } catch (e) {
    console.error(e?.stack ?? String(e));
    process.exitCode = 2;
  }
}
