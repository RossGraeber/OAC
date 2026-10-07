#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
//
// Mutation check for the security suite (#60, F11): does the suite catch a regression in
// the core mitigations it claims to prove?
//
// Copies the workspace to a work directory, then for each mutation below plants one
// regression in a copy of a core/ source file (a mitigation switched off or weakened, the
// way a careless change would), runs `cargo test -p oac-security-suite` against the copy,
// and expects at least one test to fail. An unmodified copy runs first and must pass.
// The real core/ is never touched.
//
//   node tests/security/mutation-check.mjs                    # every mutation
//   node tests/security/mutation-check.mjs --only <substring> # the mutations whose name matches
//   node tests/security/mutation-check.mjs --work-dir <dir>   # default: target/security-mutation
//
// Not part of the default CI run: each mutation recompiles core/ and the suite (a few
// minutes in all, with the work directory's target/ reused between runs). Run it when the
// suite or a core mitigation changes. Node built-ins only; cargo runs with --offline.
// Exit codes: 0 = every mutation caught and the control passes; 1 = a mutation survived
// or the control failed; 2 = usage or environment error.

import { spawnSync } from 'node:child_process';
import { cpSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');

// Each mutation: the threat row whose mitigation it breaks, the file, the exact text to
// replace (it must occur `count` times, default once) and its replacement.
export const MUTATIONS = [
  {
    name: '06 rows 1/3: signature not checked (verify_strict result ignored)',
    file: 'core/src/signing.rs',
    from: '.map_err(|_| invalid("SEC-SIG-024"))?;',
    to: '.ok();',
  },
  {
    name: '06 row 4: replay window always passes',
    file: 'core/src/replay.rs',
    from: 'if inside_replay_window(msg.envelope().created_at(), now) {',
    to: 'if true {',
  },
  {
    name: '06 row 4: every hand-off settles as not handed off (no duplicate suppression)',
    file: 'core/src/receiver.rs',
    from: 'if outcome.may_be_handed_off() {\n        reservation.handed_off();',
    to: 'if false {\n        reservation.handed_off();',
  },
  {
    name: '06 row 2: step 4 ignores a deny',
    file: 'core/src/authorization.rs',
    from: 'if !decision.permits(Kind::Deliver) {\n            return Err(refuse("SEC-AUZ-001", None));\n        }',
    to: '',
  },
  {
    name: '06 row 7: a scope grant covers every own session',
    file: 'core/src/authorization.rs',
    from: 'LocalSide::Scope(scope) => own.get(s) == Some(scope),',
    to: 'LocalSide::Scope(_) => own.contains_key(s),',
  },
  {
    name: '06 row 20: a claim on a bound session id is not refused at step 4',
    file: 'core/src/authorization.rs',
    from: 'Some(Binding::Key(bound)) if bound != &key => {',
    to: 'Some(Binding::Key(bound)) if false && bound != &key => {',
  },
  {
    name: '06 row 8: key removal keeps the key\'s grants',
    file: 'core/src/authorization.rs',
    from: '.partition(|g| g.names_key(key_id));',
    to: '.partition(|_| false);',
  },
  {
    name: '06 row 11: relay permitted for every own session',
    file: 'core/src/authorization.rs',
    from: '(self.own_sessions.contains_key(session) && self.relay.contains(session))',
    to: '(self.own_sessions.contains_key(session))',
  },
  {
    name: 'S13 presence: audience not checked',
    file: 'core/src/presence_auth.rs',
    from: 'if audience.as_str() != Some(engine.own_key_id().as_str()) {',
    to: 'if false {',
  },
  {
    name: 'S13 presence: freshness not checked',
    file: 'core/src/presence_auth.rs',
    from: '.is_some_and(|t| inside_replay_window(&t, &engine.now()));',
    to: '.is_some();',
  },
  {
    name: 'S13 receipts: wrong receiver accepted (check 4 skipped)',
    file: 'core/src/receipt_auth.rs',
    from: 'if bound != Some(Binding::Key(key)) {',
    to: 'if false {',
  },
  {
    name: 'S13 pairing: the code no longer binds the keys and nonces',
    file: 'core/src/pairing.rs',
    from: 'format!("{:06}", u64::from_be_bytes(first) % 1_000_000)',
    to: 'String::from("000000")',
  },
  {
    name: 'S13 exhaustion: no per-issuer presence quota',
    file: 'core/src/registry.rs',
    from: 'if self.held_by(issuer) >= self.per_issuer_quota {\n            let own_stale',
    to: 'if false {\n            let own_stale',
  },
  {
    name: 'S13 exhaustion: presence registry capacity not enforced',
    file: 'core/src/registry.rs',
    from: 'if self.len() < self.capacity {\n            return Ok(());\n        }\n        self.sweep(now);',
    to: 'if true {\n            return Ok(());\n        }\n        self.sweep(now);',
  },
  {
    name: 'S13 exhaustion: envelope size limit not enforced',
    file: 'core/src/envelope.rs',
    from: 'if octets.len() as u64 > limits.max_envelope_octets {',
    to: 'if false {',
  },
];

const EXCLUDE_TOP = new Set(['.git', 'target', 'node_modules', '.claude', '.agents']);

function copyWorkspace(ws) {
  rmSync(ws, { recursive: true, force: true });
  mkdirSync(ws, { recursive: true });
  // Entry by entry: the work directory may sit inside the repository's target/.
  for (const entry of readdirSync(repoRoot)) {
    if (EXCLUDE_TOP.has(entry)) continue;
    cpSync(join(repoRoot, entry), join(ws, entry), { recursive: true });
  }
}

function runSuite(ws, targetDir) {
  const r = spawnSync('cargo', ['test', '-p', 'oac-security-suite', '--offline', '--locked', '--no-fail-fast'], {
    cwd: ws,
    encoding: 'utf8',
    env: { ...process.env, CARGO_TARGET_DIR: targetDir },
    maxBuffer: 256 * 1024 * 1024,
  });
  if (r.error) throw r.error;
  const out = `${r.stdout}\n${r.stderr}`;
  const failed = [...out.matchAll(/^test (\S+) \.\.\. FAILED$/gm)].map((m) => m[1]);
  const compileError = /error(\[E\d+\])?: /.test(r.stderr) && failed.length === 0;
  return { status: r.status, failed, compileError, tail: out.split(/\r?\n/).slice(-15).join('\n') };
}

function main(argv) {
  let only = null;
  let workDir = join(repoRoot, 'target', 'security-mutation');
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--only' && argv[i + 1]) only = argv[++i];
    else if (argv[i] === '--work-dir' && argv[i + 1]) workDir = resolve(argv[++i]);
    else {
      console.error('usage: mutation-check.mjs [--only <substring>] [--work-dir <dir>]');
      return 2;
    }
  }
  const ws = join(workDir, 'ws');
  const targetDir = join(workDir, 'target');
  mkdirSync(workDir, { recursive: true });
  copyWorkspace(ws);
  try {
    const control = runSuite(ws, targetDir);
    if (control.status !== 0) {
      console.log(`FAIL  control: the unmodified suite does not pass\n${control.tail}`);
      return 1;
    }
    console.log('pass  control: the unmodified suite passes');
    let survived = 0;
    const cases = MUTATIONS.filter((m) => !only || m.name.includes(only));
    for (const m of cases) {
      const p = join(ws, m.file);
      const before = readFileSync(p, 'utf8');
      const text = before.replace(/\r\n/g, '\n');
      const count = text.split(m.from).length - 1;
      if (count !== (m.count ?? 1)) {
        console.log(`FAIL  ${m.name}: expected the text ${m.count ?? 1} time(s) in ${m.file}, found ${count}`);
        survived++;
        continue;
      }
      writeFileSync(p, text.split(m.from).join(m.to));
      try {
        const r = runSuite(ws, targetDir);
        if (r.compileError) {
          console.log(`FAIL  ${m.name}: the mutant does not compile (fix the mutation)\n${r.tail}`);
          survived++;
        } else if (r.status === 0) {
          console.log(`FAIL  ${m.name}: SURVIVED, no test failed`);
          survived++;
        } else {
          console.log(`pass  ${m.name}: caught by ${r.failed.join(', ')}`);
        }
      } finally {
        writeFileSync(p, before);
      }
    }
    console.log(`mutation check: ${cases.length - survived}/${cases.length} mutations caught`);
    return survived ? 1 : 0;
  } finally {
    rmSync(ws, { recursive: true, force: true });
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    process.exitCode = main(process.argv.slice(2));
  } catch (e) {
    console.error(e?.stack ?? String(e));
    process.exitCode = 2;
  }
}
